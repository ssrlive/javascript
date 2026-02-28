use crate::core::{InternalSlot, slot_get_chained, slot_set};
use crate::{
    JSError, Value,
    core::{
        ClosureData, DestructuringElement, EvalError, ExportSpecifier, Expr, Gc, JSObjectDataPtr, MutationContext, Statement,
        StatementKind, create_descriptor_object, new_gc_cell_ptr, object_get_key_value, object_set_key_value,
    },
    new_js_object_data,
};
use serde_json::Value as JsonValue;
use std::path::Path;

pub fn load_module<'gc>(
    mc: &MutationContext<'gc>,
    module_name: &str,
    base_path: Option<&str>,
    caller_env: Option<JSObjectDataPtr<'gc>>,
) -> Result<Value<'gc>, EvalError<'gc>> {
    // Create a new object for the module
    let module_exports = new_js_object_data(mc);

    // For demonstration, create a simple module with some exports
    if module_name == "math" {
        // Simulate loading a math module
        let pi = Value::Number(std::f64::consts::PI);
        let e = Value::Number(std::f64::consts::E);

        object_set_key_value(mc, &module_exports, "PI", &pi)?;
        object_set_key_value(mc, &module_exports, "E", &e)?;

        // Add a simple function (just return the input for now)
        let identity_func = Value::Closure(Gc::new(
            mc,
            ClosureData::new(
                &[DestructuringElement::Variable("x".to_string(), None)],
                &[Statement {
                    kind: Box::new(StatementKind::Return(Some(Expr::Var("x".to_string(), None, None)))),
                    line: 0,
                    column: 0,
                }],
                Some(module_exports),
                None,
            ),
        ));
        object_set_key_value(mc, &module_exports, "identity", &identity_func.clone())?;
        object_set_key_value(mc, &module_exports, "default", &identity_func)?;
    } else if module_name == "console" {
        // Create console module with log function
        // Create a function that directly handles console.log calls
        let log_func = Value::Function("console.log".to_string());
        object_set_key_value(mc, &module_exports, "log", &log_func)?;
    } else if module_name == "std" {
        #[cfg(feature = "std")]
        {
            let std_obj = crate::js_std::make_std_object(mc)?;
            return Ok(Value::Object(std_obj));
        }
        #[cfg(not(feature = "std"))]
        return Err(crate::raise_eval_error!("Module 'std' is not built-in (feature disabled).").into());
    } else if module_name == "os" {
        #[cfg(feature = "os")]
        {
            let os_obj = crate::js_os::make_os_object(mc)?;
            return Ok(Value::Object(os_obj));
        }
        #[cfg(not(feature = "os"))]
        return Err(crate::raise_eval_error!("Module 'os' is not built-in. Please provide it via host environment.").into());
    } else {
        // Try to load as a file
        return load_module_from_file(mc, module_name, base_path, caller_env);
    }

    Ok(Value::Object(module_exports))
}

pub fn load_module_for_dynamic_import<'gc>(
    mc: &MutationContext<'gc>,
    module_name: &str,
    base_path: Option<&str>,
    caller_env: &JSObjectDataPtr<'gc>,
) -> Result<Value<'gc>, EvalError<'gc>> {
    let module_path = resolve_module_path(module_name, base_path).map_err(EvalError::from)?;

    let cache_env = if let Some(global_val) = crate::core::env_get(caller_env, "globalThis")
        && let Value::Object(global_obj) = global_val.borrow().clone()
    {
        global_obj
    } else {
        *caller_env
    };

    let loading = get_or_create_module_loading(mc, &cache_env).map_err(EvalError::from)?;

    loop {
        let is_loading = if let Some(flag_rc) = object_get_key_value(&loading, module_path.as_str()) {
            matches!(*flag_rc.borrow(), Value::Boolean(true))
        } else {
            false
        };

        if !is_loading {
            break;
        }

        match crate::js_promise::run_event_loop(mc).map_err(EvalError::from)? {
            crate::js_promise::PollResult::Executed => continue,
            crate::js_promise::PollResult::Wait(d) => {
                std::thread::sleep(d);
                continue;
            }
            crate::js_promise::PollResult::Empty => {
                if crate::js_promise::process_runtime_pending_unhandled(mc, &cache_env, false).map_err(EvalError::from)? {
                    continue;
                }
                std::thread::yield_now();
            }
        }
    }

    preload_async_transitive_module(mc, module_name, base_path, Some(*caller_env))
}

fn expr_contains_top_level_await(expr: &Expr) -> bool {
    match expr {
        Expr::Await(_) => true,
        Expr::UnaryNeg(inner)
        | Expr::UnaryPlus(inner)
        | Expr::BitNot(inner)
        | Expr::LogicalNot(inner)
        | Expr::TypeOf(inner)
        | Expr::Void(inner)
        | Expr::Delete(inner)
        | Expr::Spread(inner)
        | Expr::Yield(Some(inner))
        | Expr::YieldStar(inner)
        | Expr::Property(inner, _)
        | Expr::OptionalProperty(inner, _)
        | Expr::OptionalPrivateMember(inner, _)
        | Expr::TaggedTemplate(inner, ..)
        | Expr::PostIncrement(inner)
        | Expr::PostDecrement(inner) => expr_contains_top_level_await(inner),
        Expr::DynamicImport(spec, options) => {
            expr_contains_top_level_await(spec) || options.as_ref().map(|e| expr_contains_top_level_await(e)).unwrap_or(false)
        }
        Expr::Assign(lhs, rhs)
        | Expr::Binary(lhs, _, rhs)
        | Expr::Conditional(lhs, _, rhs)
        | Expr::Comma(lhs, rhs)
        | Expr::LogicalAnd(lhs, rhs)
        | Expr::LogicalOr(lhs, rhs)
        | Expr::NullishCoalescing(lhs, rhs)
        | Expr::Mod(lhs, rhs)
        | Expr::Pow(lhs, rhs)
        | Expr::BitAndAssign(lhs, rhs)
        | Expr::BitOrAssign(lhs, rhs)
        | Expr::BitXorAssign(lhs, rhs)
        | Expr::LeftShiftAssign(lhs, rhs)
        | Expr::RightShiftAssign(lhs, rhs)
        | Expr::UnsignedRightShiftAssign(lhs, rhs)
        | Expr::AddAssign(lhs, rhs)
        | Expr::SubAssign(lhs, rhs)
        | Expr::MulAssign(lhs, rhs)
        | Expr::DivAssign(lhs, rhs)
        | Expr::ModAssign(lhs, rhs)
        | Expr::PowAssign(lhs, rhs)
        | Expr::LogicalAndAssign(lhs, rhs)
        | Expr::LogicalOrAssign(lhs, rhs)
        | Expr::NullishAssign(lhs, rhs) => expr_contains_top_level_await(lhs) || expr_contains_top_level_await(rhs),
        Expr::Call(callee, args) | Expr::New(callee, args) | Expr::OptionalCall(callee, args) => {
            expr_contains_top_level_await(callee) || args.iter().any(expr_contains_top_level_await)
        }
        Expr::Index(obj, idx) | Expr::OptionalIndex(obj, idx) => expr_contains_top_level_await(obj) || expr_contains_top_level_await(idx),
        Expr::Array(items) => items.iter().flatten().any(expr_contains_top_level_await),
        Expr::Object(entries) => entries
            .iter()
            .any(|(k, v, _, _)| expr_contains_top_level_await(k) || expr_contains_top_level_await(v)),
        Expr::Function(_, _, _)
        | Expr::AsyncFunction(_, _, _)
        | Expr::GeneratorFunction(_, _, _)
        | Expr::AsyncGeneratorFunction(_, _, _)
        | Expr::ArrowFunction(_, _)
        | Expr::AsyncArrowFunction(_, _) => false,
        _ => false,
    }
}

fn stmt_contains_top_level_await(stmt: &Statement) -> bool {
    match &*stmt.kind {
        StatementKind::Expr(expr) => expr_contains_top_level_await(expr),
        StatementKind::Let(decls) | StatementKind::Var(decls) => decls
            .iter()
            .any(|(_, init)| init.as_ref().is_some_and(expr_contains_top_level_await)),
        StatementKind::Const(decls) => decls.iter().any(|(_, init)| expr_contains_top_level_await(init)),
        StatementKind::Return(expr_opt) => expr_opt.as_ref().is_some_and(expr_contains_top_level_await),
        StatementKind::Throw(expr) => expr_contains_top_level_await(expr),
        StatementKind::Block(stmts) => stmts.iter().any(stmt_contains_top_level_await),
        StatementKind::If(if_stmt) => {
            expr_contains_top_level_await(&if_stmt.condition)
                || if_stmt.then_body.iter().any(stmt_contains_top_level_await)
                || if_stmt
                    .else_body
                    .as_ref()
                    .is_some_and(|b| b.iter().any(stmt_contains_top_level_await))
        }
        StatementKind::TryCatch(tc) => {
            tc.try_body.iter().any(stmt_contains_top_level_await)
                || tc.catch_body.as_ref().is_some_and(|b| b.iter().any(stmt_contains_top_level_await))
                || tc
                    .finally_body
                    .as_ref()
                    .is_some_and(|b| b.iter().any(stmt_contains_top_level_await))
        }
        StatementKind::For(for_stmt) => {
            for_stmt.init.as_ref().is_some_and(|s| stmt_contains_top_level_await(s))
                || for_stmt.test.as_ref().is_some_and(expr_contains_top_level_await)
                || for_stmt.update.as_ref().is_some_and(|s| stmt_contains_top_level_await(s))
                || for_stmt.body.iter().any(stmt_contains_top_level_await)
        }
        StatementKind::ForOf(_, _, expr, body)
        | StatementKind::ForOfExpr(_, expr, body)
        | StatementKind::ForIn(_, _, expr, body)
        | StatementKind::ForInExpr(_, expr, body)
        | StatementKind::ForInDestructuringObject(_, _, expr, body)
        | StatementKind::ForInDestructuringArray(_, _, expr, body)
        | StatementKind::ForOfDestructuringObject(_, _, expr, body)
        | StatementKind::ForOfDestructuringArray(_, _, expr, body) => {
            expr_contains_top_level_await(expr) || body.iter().any(stmt_contains_top_level_await)
        }
        StatementKind::ForAwaitOf(..)
        | StatementKind::ForAwaitOfExpr(..)
        | StatementKind::ForAwaitOfDestructuringObject(..)
        | StatementKind::ForAwaitOfDestructuringArray(..) => true,
        StatementKind::While(cond, body) | StatementKind::DoWhile(body, cond) => {
            expr_contains_top_level_await(cond) || body.iter().any(stmt_contains_top_level_await)
        }
        StatementKind::Switch(sw) => {
            expr_contains_top_level_await(&sw.expr)
                || sw.cases.iter().any(|c| match c {
                    crate::core::SwitchCase::Case(expr, stmts) => {
                        expr_contains_top_level_await(expr) || stmts.iter().any(stmt_contains_top_level_await)
                    }
                    crate::core::SwitchCase::Default(stmts) => stmts.iter().any(stmt_contains_top_level_await),
                })
        }
        StatementKind::With(expr, body) => expr_contains_top_level_await(expr) || body.iter().any(stmt_contains_top_level_await),
        StatementKind::Label(_, stmt) => stmt_contains_top_level_await(stmt),
        StatementKind::Export(_, inner_stmt, _) => inner_stmt.as_ref().is_some_and(|stmt| stmt_contains_top_level_await(stmt)),
        StatementKind::FunctionDeclaration(_, _, _, _, _)
        | StatementKind::Class(_)
        | StatementKind::Import(_, _)
        | StatementKind::Break(_)
        | StatementKind::Continue(_)
        | StatementKind::Debugger
        | StatementKind::Assign(_, _)
        | StatementKind::LetDestructuringArray(_, _)
        | StatementKind::VarDestructuringArray(_, _)
        | StatementKind::ConstDestructuringArray(_, _)
        | StatementKind::LetDestructuringObject(_, _)
        | StatementKind::VarDestructuringObject(_, _)
        | StatementKind::ConstDestructuringObject(_, _) => false,
        StatementKind::Using(decls) => decls.iter().any(|(_, expr)| expr_contains_top_level_await(expr)),
        StatementKind::AwaitUsing(_) => true,
    }
}

#[allow(dead_code)]
pub fn module_contains_top_level_await(module_name: &str, base_path: Option<&str>) -> Result<bool, JSError> {
    let module_path = resolve_module_path(module_name, base_path)?;
    if module_path.ends_with(".json") {
        return Ok(false);
    }
    let content = crate::core::read_script_file(&module_path)?;
    let tokens = crate::core::tokenize(&content)?;
    let mut index = 0;
    crate::core::push_await_context();
    let parsed = crate::core::parse_statements(&tokens, &mut index);
    crate::core::pop_await_context();
    let statements = parsed?;
    Ok(statements.iter().any(stmt_contains_top_level_await))
}

fn module_has_async_transitive_from_path(module_path: &str, seen: &mut std::collections::HashSet<String>) -> Result<bool, JSError> {
    if !seen.insert(module_path.to_string()) {
        return Ok(false);
    }

    if module_path.ends_with(".json") {
        return Ok(false);
    }

    let content = crate::core::read_script_file(module_path)?;
    let tokens = crate::core::tokenize(&content)?;
    let mut index = 0;
    crate::core::push_await_context();
    let parsed = crate::core::parse_statements(&tokens, &mut index);
    crate::core::pop_await_context();
    let statements = parsed?;

    if statements.iter().any(stmt_contains_top_level_await) {
        return Ok(true);
    }

    for stmt in &statements {
        let source_opt = match &*stmt.kind {
            StatementKind::Import(_, source) => Some(source.as_str()),
            StatementKind::Export(_, _, Some(source)) => Some(source.as_str()),
            _ => None,
        };

        if let Some(source) = source_opt
            && let Ok(req_path) = resolve_module_path(source, Some(module_path))
            && module_has_async_transitive_from_path(req_path.as_str(), seen)?
        {
            return Ok(true);
        }
    }

    Ok(false)
}

fn gather_async_transitive_from_path(
    module_path: &str,
    seen: &mut std::collections::HashSet<String>,
    out: &mut Vec<String>,
) -> Result<(), JSError> {
    if !seen.insert(module_path.to_string()) {
        return Ok(());
    }

    if module_path.ends_with(".json") {
        return Ok(());
    }

    let content = crate::core::read_script_file(module_path)?;
    let tokens = crate::core::tokenize(&content)?;
    let mut index = 0;
    crate::core::push_await_context();
    let parsed = crate::core::parse_statements(&tokens, &mut index);
    crate::core::pop_await_context();
    let statements = parsed?;

    if statements.iter().any(stmt_contains_top_level_await) {
        out.push(module_path.to_string());
        return Ok(());
    }

    for stmt in &statements {
        let source_opt = match &*stmt.kind {
            StatementKind::Import(_, source) => Some(source.as_str()),
            StatementKind::Export(_, _, Some(source)) => Some(source.as_str()),
            _ => None,
        };

        if let Some(source) = source_opt
            && let Ok(req_path) = resolve_module_path(source, Some(module_path))
        {
            gather_async_transitive_from_path(req_path.as_str(), seen, out)?;
        }
    }

    Ok(())
}

#[allow(dead_code)]
pub fn module_has_async_transitive_dependencies(module_name: &str, base_path: Option<&str>) -> Result<bool, JSError> {
    let module_path = resolve_module_path(module_name, base_path)?;
    let mut seen = std::collections::HashSet::new();
    module_has_async_transitive_from_path(module_path.as_str(), &mut seen)
}

#[allow(dead_code)]
pub fn module_has_direct_async_dependency(module_name: &str, base_path: Option<&str>) -> Result<bool, JSError> {
    let module_path = resolve_module_path(module_name, base_path)?;
    let requests = module_requested_modules(module_path.as_str())?;

    for req in requests {
        if let Ok(req_path) = resolve_module_path(req.as_str(), Some(module_path.as_str())) {
            let mut seen = std::collections::HashSet::new();
            if module_has_async_transitive_from_path(req_path.as_str(), &mut seen)? {
                return Ok(true);
            }
        }
    }

    Ok(false)
}

pub fn gather_async_transitive_dependencies(module_name: &str, base_path: Option<&str>) -> Result<Vec<String>, JSError> {
    let module_path = resolve_module_path(module_name, base_path)?;
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    gather_async_transitive_from_path(module_path.as_str(), &mut seen, &mut out)?;
    Ok(out)
}

fn module_requested_modules(module_path: &str) -> Result<Vec<String>, JSError> {
    let content = crate::core::read_script_file(module_path)?;
    let tokens = crate::core::tokenize(&content)?;
    let mut index = 0;
    crate::core::push_await_context();
    let parsed = crate::core::parse_statements(&tokens, &mut index);
    crate::core::pop_await_context();
    let statements = parsed?;

    let mut requests = Vec::new();
    for stmt in &statements {
        match &*stmt.kind {
            StatementKind::Import(_, source) => requests.push(source.clone()),
            StatementKind::Export(_, _, Some(source)) => requests.push(source.clone()),
            _ => {}
        }
    }

    Ok(requests)
}

fn is_module_loading_in_env_chain<'gc>(env: &JSObjectDataPtr<'gc>, module_path: &str) -> bool {
    let mut cur = Some(*env);
    while let Some(e) = cur {
        if let Some(loading_val) = slot_get_chained(&e, &InternalSlot::ModuleLoading)
            && let Value::Object(loading_obj) = loading_val.borrow().clone()
            && let Some(flag_rc) = object_get_key_value(&loading_obj, module_path)
            && matches!(*flag_rc.borrow(), Value::Boolean(true))
        {
            return true;
        }

        if let Some(global_val) = object_get_key_value(&e, "globalThis")
            && let Value::Object(global_obj) = global_val.borrow().clone()
            && let Some(loading_val) = slot_get_chained(&global_obj, &InternalSlot::ModuleLoading)
            && let Value::Object(loading_obj) = loading_val.borrow().clone()
            && let Some(flag_rc) = object_get_key_value(&loading_obj, module_path)
            && matches!(*flag_rc.borrow(), Value::Boolean(true))
        {
            return true;
        }

        cur = e.borrow().prototype;
    }
    false
}

#[allow(dead_code)]
fn any_module_loading_in_env_chain<'gc>(env: &JSObjectDataPtr<'gc>) -> bool {
    let mut cur = Some(*env);
    while let Some(e) = cur {
        if let Some(loading_val) = slot_get_chained(&e, &InternalSlot::ModuleLoading)
            && let Value::Object(loading_obj) = loading_val.borrow().clone()
        {
            for flag in loading_obj.borrow().properties.values() {
                if matches!(*flag.borrow(), Value::Boolean(true)) {
                    return true;
                }
            }
        }

        if let Some(global_val) = object_get_key_value(&e, "globalThis")
            && let Value::Object(global_obj) = global_val.borrow().clone()
            && let Some(loading_val) = slot_get_chained(&global_obj, &InternalSlot::ModuleLoading)
            && let Value::Object(loading_obj) = loading_val.borrow().clone()
        {
            for flag in loading_obj.borrow().properties.values() {
                if matches!(*flag.borrow(), Value::Boolean(true)) {
                    return true;
                }
            }
        }

        cur = e.borrow().prototype;
    }
    false
}

fn module_has_loading_dependency(
    module_path: &str,
    cache_env: &JSObjectDataPtr<'_>,
    seen: &mut std::collections::HashSet<String>,
) -> Result<bool, JSError> {
    if !seen.insert(module_path.to_string()) {
        return Ok(false);
    }

    for req in module_requested_modules(module_path)? {
        if let Ok(req_path) = resolve_module_path(req.as_str(), Some(module_path)) {
            if is_module_loading_in_env_chain(cache_env, req_path.as_str()) {
                return Ok(true);
            }
            if module_has_loading_dependency(req_path.as_str(), cache_env, seen)? {
                return Ok(true);
            }
        }
    }

    Ok(false)
}

fn is_module_async_pending_in_env_chain<'gc>(env: &JSObjectDataPtr<'gc>, module_path: &str) -> bool {
    let mut cur = Some(*env);
    while let Some(e) = cur {
        if let Some(pending_val) = slot_get_chained(&e, &InternalSlot::ModuleAsyncPending)
            && let Value::Object(pending_obj) = pending_val.borrow().clone()
            && let Some(flag_rc) = object_get_key_value(&pending_obj, module_path)
            && matches!(*flag_rc.borrow(), Value::Boolean(true))
        {
            return true;
        }

        if let Some(global_val) = object_get_key_value(&e, "globalThis")
            && let Value::Object(global_obj) = global_val.borrow().clone()
            && let Some(pending_val) = slot_get_chained(&global_obj, &InternalSlot::ModuleAsyncPending)
            && let Value::Object(pending_obj) = pending_val.borrow().clone()
            && let Some(flag_rc) = object_get_key_value(&pending_obj, module_path)
            && matches!(*flag_rc.borrow(), Value::Boolean(true))
        {
            return true;
        }

        cur = e.borrow().prototype;
    }
    false
}

fn module_has_async_pending_dependency(
    module_path: &str,
    cache_env: &JSObjectDataPtr<'_>,
    seen: &mut std::collections::HashSet<String>,
) -> Result<bool, JSError> {
    if !seen.insert(module_path.to_string()) {
        return Ok(false);
    }

    for req in module_requested_modules(module_path)? {
        if let Ok(req_path) = resolve_module_path(req.as_str(), Some(module_path)) {
            if is_module_async_pending_in_env_chain(cache_env, req_path.as_str()) {
                return Ok(true);
            }
            if module_has_async_pending_dependency(req_path.as_str(), cache_env, seen)? {
                return Ok(true);
            }
        }
    }

    Ok(false)
}

pub(crate) fn deferred_preload_ready_now<'gc>(module_path: &str, env: &JSObjectDataPtr<'gc>) -> Result<bool, JSError> {
    let cache_env = resolve_cache_env(Some(*env)).unwrap_or(*env);
    let is_loading = is_module_loading_in_env_chain(&cache_env, module_path);
    let has_loading_dep = module_has_loading_dependency(module_path, &cache_env, &mut std::collections::HashSet::new())?;
    let has_async_pending_dep = module_has_async_pending_dependency(module_path, &cache_env, &mut std::collections::HashSet::new())?;
    Ok(!is_loading && !has_loading_dep && !has_async_pending_dep)
}

fn load_module_from_file<'gc>(
    mc: &MutationContext<'gc>,
    module_name: &str,
    base_path: Option<&str>,
    caller_env: Option<JSObjectDataPtr<'gc>>,
) -> Result<Value<'gc>, EvalError<'gc>> {
    load_module_from_file_with_mode(mc, module_name, base_path, caller_env, false)
}

fn load_module_from_file_with_mode<'gc>(
    mc: &MutationContext<'gc>,
    module_name: &str,
    base_path: Option<&str>,
    caller_env: Option<JSObjectDataPtr<'gc>>,
    preload_tla_async: bool,
) -> Result<Value<'gc>, EvalError<'gc>> {
    // Resolve the module path
    let module_path = resolve_module_path(module_name, base_path).map_err(EvalError::from)?;
    let mark_async_pending = preload_tla_async && module_contains_top_level_await(module_name, base_path).unwrap_or(false);

    let cache_env = resolve_cache_env(caller_env);
    if let Some(cache_env) = cache_env {
        let cache = get_or_create_module_cache(mc, &cache_env)?;
        let eval_errors = get_or_create_module_eval_errors(mc, &cache_env)?;
        let async_pending = get_or_create_module_async_pending(mc, &cache_env)?;

        if let Some(err_rc) = object_get_key_value(&eval_errors, module_path.as_str()) {
            return Err(EvalError::Throw(err_rc.borrow().clone(), None, None));
        }

        if let Some(val_rc) = object_get_key_value(&cache, module_path.as_str()) {
            return Ok(val_rc.borrow().clone());
        }

        let loading = get_or_create_module_loading(mc, &cache_env)?;
        if let Some(flag_rc) = object_get_key_value(&loading, module_path.as_str())
            && matches!(*flag_rc.borrow(), Value::Boolean(true))
        {
            return Err(crate::raise_syntax_error!("Circular module import").into());
        }

        object_set_key_value(mc, &loading, module_path.as_str(), &Value::Boolean(true))?;
        if mark_async_pending {
            object_set_key_value(mc, &async_pending, module_path.as_str(), &Value::Boolean(true))?;
        }

        let module_exports = new_js_object_data(mc);
        object_set_key_value(mc, &cache, module_path.as_str(), &Value::Object(module_exports))?;

        // Read the file
        let content = crate::core::read_script_file(&module_path).map_err(EvalError::from)?;

        if module_path.ends_with(".json") {
            let json_val: JsonValue =
                serde_json::from_str(&content).map_err(|e| EvalError::from(raise_syntax_error!(format!("Invalid JSON module: {e}"))))?;
            let js_default = json_to_js_value(mc, &json_val, caller_env.as_ref())?;
            object_set_key_value(mc, &module_exports, "default", &js_default)?;

            let value = Value::Object(module_exports);
            object_set_key_value(mc, &cache, module_path.as_str(), &value.clone())?;
            object_set_key_value(mc, &loading, module_path.as_str(), &Value::Boolean(false))?;
            object_set_key_value(mc, &async_pending, module_path.as_str(), &Value::Boolean(false))?;
            return Ok(value);
        }

        // Execute the module and get the final module value
        let value = match execute_module(mc, &content, &module_path, caller_env, Some(module_exports), preload_tla_async) {
            Ok(v) => v,
            Err(EvalError::Throw(throw_val, line, column)) => {
                object_set_key_value(mc, &eval_errors, module_path.as_str(), &throw_val)?;
                object_set_key_value(mc, &loading, module_path.as_str(), &Value::Boolean(false))?;
                object_set_key_value(mc, &async_pending, module_path.as_str(), &Value::Boolean(false))?;
                return Err(EvalError::Throw(throw_val, line, column));
            }
            Err(e) => {
                object_set_key_value(mc, &loading, module_path.as_str(), &Value::Boolean(false))?;
                object_set_key_value(mc, &async_pending, module_path.as_str(), &Value::Boolean(false))?;
                return Err(e);
            }
        };

        object_set_key_value(mc, &cache, module_path.as_str(), &value)?;
        object_set_key_value(mc, &loading, module_path.as_str(), &Value::Boolean(false))?;
        return Ok(value);
    }

    // Read the file
    let content = crate::core::read_script_file(&module_path).map_err(EvalError::from)?;

    if module_path.ends_with(".json") {
        let json_val: JsonValue =
            serde_json::from_str(&content).map_err(|e| EvalError::from(raise_syntax_error!(format!("Invalid JSON module: {e}"))))?;
        let module_exports = new_js_object_data(mc);
        let js_default = json_to_js_value(mc, &json_val, caller_env.as_ref())?;
        object_set_key_value(mc, &module_exports, "default", &js_default)?;
        return Ok(Value::Object(module_exports));
    }

    // Execute the module and get the final module value
    execute_module(mc, &content, &module_path, caller_env, None, preload_tla_async)
}

#[allow(dead_code)]
pub fn preload_async_transitive_module<'gc>(
    mc: &MutationContext<'gc>,
    module_name: &str,
    base_path: Option<&str>,
    caller_env: Option<JSObjectDataPtr<'gc>>,
) -> Result<Value<'gc>, EvalError<'gc>> {
    load_module_from_file_with_mode(mc, module_name, base_path, caller_env, true)
}

fn json_to_js_value<'gc>(
    mc: &MutationContext<'gc>,
    json: &JsonValue,
    caller_env: Option<&JSObjectDataPtr<'gc>>,
) -> Result<Value<'gc>, EvalError<'gc>> {
    Ok(match json {
        JsonValue::Null => Value::Null,
        JsonValue::Bool(b) => Value::Boolean(*b),
        JsonValue::Number(n) => Value::Number(n.as_f64().unwrap_or(f64::NAN)),
        JsonValue::String(s) => Value::String(crate::unicode::utf8_to_utf16(s)),
        JsonValue::Array(items) => {
            let arr_obj = if let Some(env) = caller_env {
                crate::js_array::create_array(mc, env).map_err(EvalError::from)?
            } else {
                new_js_object_data(mc)
            };
            for (idx, item) in items.iter().enumerate() {
                let v = json_to_js_value(mc, item, caller_env)?;
                object_set_key_value(mc, &arr_obj, idx, &v).map_err(EvalError::from)?;
            }
            object_set_key_value(mc, &arr_obj, "length", &Value::Number(items.len() as f64)).map_err(EvalError::from)?;
            Value::Object(arr_obj)
        }
        JsonValue::Object(map) => {
            let obj = new_js_object_data(mc);
            if let Some(env) = caller_env {
                let _ = crate::core::set_internal_prototype_from_constructor(mc, &obj, env, "Object");
            }
            for (k, v) in map.iter() {
                let vv = json_to_js_value(mc, v, caller_env)?;
                object_set_key_value(mc, &obj, k.as_str(), &vv).map_err(EvalError::from)?;
            }
            Value::Object(obj)
        }
    })
}

pub(crate) fn resolve_module_path(module_name: &str, base_path: Option<&str>) -> Result<String, JSError> {
    let path = Path::new(module_name);

    // If it's an absolute path or starts with ./ or ../, treat as file path
    if path.is_absolute() || module_name.starts_with("./") || module_name.starts_with("../") {
        // Trim a leading "./" so joining with the crate root doesn't produce
        // a path containing a literal './' segment which may cause
        // `exists()` to fail on some platforms/environments.
        let mut full_path = if let Some(base) = base_path {
            // Use the directory containing the base file as the base directory
            Path::new(base).parent().unwrap_or(Path::new(".")).join(module_name)
        } else {
            // Use current working directory as base when no base_path is provided
            std::env::current_dir()
                .map_err(|e| crate::raise_eval_error!(format!("Failed to get current directory: {e}")))?
                .join(module_name)
        };

        // Add .js extension if not present
        if full_path.extension().is_none() {
            full_path.set_extension("js");
        }

        // Canonicalize the path
        match full_path.canonicalize() {
            Ok(canonical) => Ok(canonical.to_string_lossy().to_string()),
            Err(_) => Err(crate::raise_eval_error!(format!("Module file not found: {}", full_path.display()))),
        }
    } else {
        // For now, treat relative paths as relative to current directory
        let mut full_path = Path::new(module_name).to_path_buf();
        if full_path.extension().is_none() {
            full_path.set_extension("js");
        }

        match full_path.canonicalize() {
            Ok(canonical) => Ok(canonical.to_string_lossy().to_string()),
            Err(_) => Err(crate::raise_eval_error!(format!("Module file not found: {}", full_path.display()))),
        }
    }
}

/// Result of the ResolveExport algorithm (spec 16.2.1.6.3)
#[derive(Debug)]
enum ExportResolution {
    /// Export resolved to a specific binding in a specific module
    Found { module_path: String, binding_name: String },
    /// Resolution not found (circular or simply absent)
    Null,
    /// Ambiguous resolution from multiple star exports
    Ambiguous,
}

/// Implements the ResolveExport abstract operation (spec 16.2.1.6.3).
/// Given a module file path and an export name, determines whether the export
/// can be resolved to a unique binding, is ambiguous, or is absent/circular.
fn resolve_export(module_path: &str, export_name: &str, resolve_set: &mut Vec<(String, String)>) -> Result<ExportResolution, JSError> {
    // Step 2: Circular reference check
    if resolve_set.iter().any(|(m, n)| m == module_path && n == export_name) {
        return Ok(ExportResolution::Null);
    }

    // Step 3: Add to resolve set
    resolve_set.push((module_path.to_string(), export_name.to_string()));

    // Parse the module
    let content = std::fs::read_to_string(module_path)
        .map_err(|e| crate::raise_eval_error!(format!("Failed to read module '{}': {e}", module_path)))?;
    let tokens = crate::core::tokenize(&content)?;
    let mut index = 0;
    crate::core::push_await_context();
    let parse_result = crate::core::parse_statements(&tokens, &mut index);
    crate::core::pop_await_context();
    let statements = parse_result?;

    // Collect imported local names to distinguish local vs imported bindings
    let mut imported_locals = std::collections::HashSet::new();
    for stmt in &statements {
        if let StatementKind::Import(specifiers, _) = &*stmt.kind {
            for spec in specifiers {
                match spec {
                    crate::core::ImportSpecifier::Named(name, alias) => {
                        imported_locals.insert(alias.as_ref().unwrap_or(name).clone());
                    }
                    crate::core::ImportSpecifier::Default(name) | crate::core::ImportSpecifier::Namespace(name) => {
                        imported_locals.insert(name.clone());
                    }
                }
            }
        }
    }

    // Step 5: Check local export entries
    for stmt in &statements {
        if let StatementKind::Export(specifiers, inner_stmt, source) = &*stmt.kind {
            if source.is_some() {
                continue; // re-exports are not local
            }
            // Check inner declarations (export var/let/const/function/class)
            if let Some(inner) = inner_stmt {
                let found = match &*inner.kind {
                    StatementKind::Var(decls) | StatementKind::Let(decls) => decls.iter().any(|(name, _)| name == export_name),
                    StatementKind::Const(decls) => decls.iter().any(|(name, _)| name == export_name),
                    StatementKind::FunctionDeclaration(name, ..) => name == export_name,
                    StatementKind::Class(class_def) => class_def.name == export_name,
                    _ => false,
                };
                if found {
                    return Ok(ExportResolution::Found {
                        module_path: module_path.to_string(),
                        binding_name: export_name.to_string(),
                    });
                }
            }
            // Check `export default expr` (via Default specifier)
            for spec in specifiers {
                match spec {
                    ExportSpecifier::Default(_) if export_name == "default" => {
                        return Ok(ExportResolution::Found {
                            module_path: module_path.to_string(),
                            binding_name: "default".to_string(),
                        });
                    }
                    ExportSpecifier::Named(local_name, alias) => {
                        let out_name = alias.as_ref().unwrap_or(local_name);
                        if out_name == export_name && !imported_locals.contains(local_name) {
                            return Ok(ExportResolution::Found {
                                module_path: module_path.to_string(),
                                binding_name: local_name.clone(),
                            });
                        }
                    }
                    _ => {}
                }
            }
        }
    }

    // Step 6: Check indirect export entries
    for stmt in &statements {
        if let StatementKind::Export(specifiers, _, Some(source)) = &*stmt.kind {
            for spec in specifiers {
                match spec {
                    ExportSpecifier::Named(import_name, alias) => {
                        let out_name = alias.as_ref().unwrap_or(import_name);
                        if out_name == export_name {
                            let resolved_path = resolve_module_path(source, Some(module_path))?;
                            return resolve_export(&resolved_path, import_name, resolve_set);
                        }
                    }
                    ExportSpecifier::Namespace(name) => {
                        if name == export_name {
                            return Ok(ExportResolution::Found {
                                module_path: module_path.to_string(),
                                binding_name: "*namespace*".to_string(),
                            });
                        }
                    }
                    _ => {} // Star handled below
                }
            }
        }
        // Also handle `export { x }` where x is imported (indirect re-export)
        if let StatementKind::Export(specifiers, _, None) = &*stmt.kind {
            for spec in specifiers {
                if let ExportSpecifier::Named(local_name, alias) = spec {
                    let out_name = alias.as_ref().unwrap_or(local_name);
                    if out_name == export_name && imported_locals.contains(local_name) {
                        // Find the import source for local_name
                        for imp_stmt in &statements {
                            if let StatementKind::Import(imp_specs, imp_source) = &*imp_stmt.kind {
                                for imp_spec in imp_specs {
                                    let (imp_name, local) = match imp_spec {
                                        crate::core::ImportSpecifier::Named(name, al) => {
                                            (name.clone(), al.as_ref().unwrap_or(name).clone())
                                        }
                                        crate::core::ImportSpecifier::Default(name) => ("default".to_string(), name.clone()),
                                        crate::core::ImportSpecifier::Namespace(name) => ("*".to_string(), name.clone()),
                                    };
                                    if local == *local_name {
                                        let resolved_path = resolve_module_path(imp_source, Some(module_path))?;
                                        if imp_name == "*" {
                                            return Ok(ExportResolution::Found {
                                                module_path: resolved_path,
                                                binding_name: "*namespace*".to_string(),
                                            });
                                        }
                                        return resolve_export(&resolved_path, &imp_name, resolve_set);
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    // Step 7: "default" is not provided by export * from "mod"
    if export_name == "default" {
        return Ok(ExportResolution::Null);
    }

    // Steps 8-9: Check star export entries
    let mut star_resolution: Option<(String, String)> = None; // (module_path, binding_name)
    for stmt in &statements {
        if let StatementKind::Export(specifiers, _, Some(source)) = &*stmt.kind
            && specifiers.iter().any(|s| matches!(s, ExportSpecifier::Star))
        {
            let resolved_path = resolve_module_path(source, Some(module_path))?;
            let resolution = resolve_export(&resolved_path, export_name, resolve_set)?;
            match resolution {
                ExportResolution::Ambiguous => return Ok(ExportResolution::Ambiguous),
                ExportResolution::Found {
                    module_path: m,
                    binding_name: b,
                } => {
                    if let Some((ref sm, ref sb)) = star_resolution {
                        if sm != &m || sb != &b {
                            return Ok(ExportResolution::Ambiguous);
                        }
                    } else {
                        star_resolution = Some((m, b));
                    }
                }
                ExportResolution::Null => {}
            }
        }
    }

    match star_resolution {
        Some((m, b)) => Ok(ExportResolution::Found {
            module_path: m,
            binding_name: b,
        }),
        None => Ok(ExportResolution::Null),
    }
}

/// Validate indirect export entries for a module (spec 16.2.1.6.4 InitializeEnvironment step 5).
/// For each `export { name } from 'source'`, verify that the export can be resolved
/// in the source module (not ambiguous or circular).
fn validate_indirect_export_entries(statements: &[Statement], module_path: &str) -> Result<(), JSError> {
    for stmt in statements {
        if let StatementKind::Export(specifiers, _, Some(source)) = &*stmt.kind {
            for spec in specifiers {
                if let ExportSpecifier::Named(import_name, alias) = spec {
                    let out_name = alias.as_ref().unwrap_or(import_name);
                    // Seed resolve_set with the current module to detect circular back-references
                    let mut resolve_set = vec![(module_path.to_string(), out_name.clone())];
                    let resolved_path = resolve_module_path(source, Some(module_path))?;
                    let resolution = resolve_export(&resolved_path, import_name, &mut resolve_set)?;
                    match resolution {
                        ExportResolution::Null | ExportResolution::Ambiguous => {
                            return Err(crate::raise_syntax_error!(format!(
                                "The requested module '{}' does not provide an export named '{}'",
                                source, import_name
                            )));
                        }
                        ExportResolution::Found { .. } => {}
                    }
                }
            }
        }
    }
    Ok(())
}

/// Validate module-level declarations per ECMAScript spec early errors:
/// - In module code, function/generator/async declarations are *lexically* scoped
/// - No name may appear in both LexicallyDeclaredNames and VarDeclaredNames
/// - LexicallyDeclaredNames must not contain duplicates
fn validate_module_declarations(statements: &[Statement]) -> Result<(), JSError> {
    use std::collections::HashSet;

    let mut var_names = HashSet::new();
    let mut lex_names = HashSet::new();

    fn collect_binding_names_from_destr_elements(elems: &[DestructuringElement], names: &mut HashSet<String>) {
        for elem in elems {
            match elem {
                DestructuringElement::Variable(name, _) => {
                    names.insert(name.clone());
                }
                DestructuringElement::Rest(name) => {
                    names.insert(name.clone());
                }
                DestructuringElement::RestPattern(inner) => {
                    collect_binding_names_from_destr_elements(std::slice::from_ref(inner), names);
                }
                DestructuringElement::NestedArray(inner, _) => {
                    collect_binding_names_from_destr_elements(inner, names);
                }
                DestructuringElement::NestedObject(inner, _) => {
                    collect_binding_names_from_destr_elements(inner, names);
                }
                DestructuringElement::Property(_, inner) => {
                    collect_binding_names_from_destr_elements(std::slice::from_ref(inner), names);
                }
                DestructuringElement::ComputedProperty(_, inner) => {
                    collect_binding_names_from_destr_elements(std::slice::from_ref(inner), names);
                }
                DestructuringElement::Empty => {}
            }
        }
    }

    fn collect_binding_names_from_obj_destr(elems: &[crate::core::ObjectDestructuringElement], names: &mut HashSet<String>) {
        for elem in elems {
            match elem {
                crate::core::ObjectDestructuringElement::Property { value, .. }
                | crate::core::ObjectDestructuringElement::ComputedProperty { value, .. } => {
                    collect_binding_names_from_destr_elements(std::slice::from_ref(value), names);
                }
                crate::core::ObjectDestructuringElement::Rest(name) => {
                    names.insert(name.clone());
                }
            }
        }
    }

    for stmt in statements {
        // Unwrap Export to get the inner declaration
        let inner = if let StatementKind::Export(_, Some(inner_stmt), _) = &*stmt.kind {
            &*inner_stmt.kind
        } else {
            &*stmt.kind
        };

        match inner {
            // Var declarations go into VarDeclaredNames
            StatementKind::Var(decls) => {
                for (name, _) in decls {
                    var_names.insert(name.clone());
                }
            }
            StatementKind::VarDestructuringArray(elems, _) => {
                collect_binding_names_from_destr_elements(elems, &mut var_names);
            }
            StatementKind::VarDestructuringObject(elems, _) => {
                collect_binding_names_from_obj_destr(elems, &mut var_names);
            }

            // In module code, function/class/let/const declarations are LexicallyDeclaredNames
            StatementKind::FunctionDeclaration(name, ..) => {
                lex_names.insert(name.clone());
            }
            StatementKind::Class(class_def) => {
                if !class_def.name.is_empty() {
                    lex_names.insert(class_def.name.clone());
                }
            }
            StatementKind::Let(decls) => {
                for (name, _) in decls {
                    lex_names.insert(name.clone());
                }
            }
            StatementKind::Const(decls) => {
                for (name, _) in decls {
                    lex_names.insert(name.clone());
                }
            }
            StatementKind::LetDestructuringArray(elems, _) | StatementKind::ConstDestructuringArray(elems, _) => {
                collect_binding_names_from_destr_elements(elems, &mut lex_names);
            }
            StatementKind::LetDestructuringObject(elems, _) | StatementKind::ConstDestructuringObject(elems, _) => {
                collect_binding_names_from_obj_destr(elems, &mut lex_names);
            }

            _ => {}
        }
    }

    // Check for names appearing in both LexicallyDeclaredNames and VarDeclaredNames
    for name in &lex_names {
        if var_names.contains(name) {
            return Err(crate::raise_syntax_error!(format!(
                "Identifier '{}' has already been declared",
                name
            )));
        }
    }

    Ok(())
}

fn execute_module<'gc>(
    mc: &MutationContext<'gc>,
    content: &str,
    module_path: &str,
    caller_env: Option<JSObjectDataPtr<'gc>>,
    module_exports_override: Option<JSObjectDataPtr<'gc>>,
    preload_tla_async: bool,
) -> Result<Value<'gc>, EvalError<'gc>> {
    // Create module exports object
    let module_exports = module_exports_override.unwrap_or_else(|| new_js_object_data(mc));

    // Create a module environment
    let env = new_js_object_data(mc);
    env.borrow_mut(mc).is_function_scope = true;

    // Record a module path on the module environment so stack frames / errors can include it
    // Store as `__filepath` similarly to `evaluate_script`.
    let val = Value::String(crate::unicode::utf8_to_utf16(module_path));
    slot_set(mc, &env, InternalSlot::Filepath, &val);

    // Add exports object to the environment
    env.borrow_mut(mc).insert(
        crate::core::PropertyKey::String("exports".to_string()),
        new_gc_cell_ptr(mc, Value::Object(module_exports)),
    );

    // Add module object with exports
    let module_obj = new_js_object_data(mc);
    module_obj.borrow_mut(mc).insert(
        crate::core::PropertyKey::String("exports".to_string()),
        new_gc_cell_ptr(mc, Value::Object(module_exports)),
    );
    env.borrow_mut(mc).insert(
        crate::core::PropertyKey::String("module".to_string()),
        new_gc_cell_ptr(mc, Value::Object(module_obj)),
    );

    // In ECMAScript modules, top-level `this` is `undefined`.
    object_set_key_value(mc, &env, "this", &Value::Undefined)?;

    // Create and store import.meta object for this module
    let import_meta = new_js_object_data(mc);
    // Provide a 'url' property referencing the module path; leave as raw path string
    object_set_key_value(mc, &import_meta, "url", &Value::String(crate::unicode::utf8_to_utf16(module_path)))?;
    // Store the import.meta object on the module environment under a hidden key
    slot_set(mc, &env, InternalSlot::ImportMeta, &Value::Object(import_meta));

    if caller_env.is_none() {
        // Initialize global constructors for standalone module execution
        crate::core::initialize_global_constructors(mc, &env)?;
        object_set_key_value(mc, &env, "globalThis", &crate::core::Value::Object(env))?;
    } else if let Some(caller) = caller_env {
        let global_obj = if let Some(global_val) = crate::core::env_get(&caller, "globalThis") {
            match global_val.borrow().clone() {
                Value::Object(global_obj) => global_obj,
                _ => caller,
            }
        } else {
            caller
        };
        // Modules should resolve globals through the realm global object, not
        // through the importing module's lexical environment.
        env.borrow_mut(mc).prototype = Some(global_obj);
        object_set_key_value(mc, &env, "globalThis", &crate::core::Value::Object(global_obj))?;
    }

    // Parse and execute the module content
    let tokens = crate::core::tokenize(content).map_err(EvalError::from)?;
    let mut index = 0;
    crate::core::push_await_context();
    let parse_result = crate::core::parse_statements(&tokens, &mut index);
    crate::core::pop_await_context();
    let statements = parse_result.map_err(EvalError::from)?;

    // Module early error checks per spec:
    // - LexicallyDeclaredNames must not contain any duplicates
    // - No name may appear in both LexicallyDeclaredNames and VarDeclaredNames
    // In module code, function/class declarations are lexically scoped (not var-scoped).
    validate_module_declarations(&statements).map_err(EvalError::from)?;

    // Validate indirect export entries: for each `export { x } from '...'`,
    // verify that `x` can be resolved in the source module (not ambiguous or circular).
    validate_indirect_export_entries(&statements, module_path).map_err(EvalError::from)?;

    if preload_tla_async && let Some(first_tla_idx) = statements.iter().position(stmt_contains_top_level_await) {
        if first_tla_idx > 0 {
            crate::core::evaluate_statements(mc, &env, &statements[..first_tla_idx])?;
        }

        if first_tla_idx + 1 < statements.len() {
            let tail_stmts = statements[(first_tla_idx + 1)..].to_vec();
            let cont = Value::Closure(Gc::new(mc, ClosureData::new(&[], &tail_stmts, Some(env), None)));
            let (p, resolve, _) = crate::js_promise::create_promise_capability(mc, &env).map_err(EvalError::from)?;
            crate::js_promise::call_function(mc, &resolve, &[Value::Undefined], &env)?;
            crate::js_promise::perform_promise_then(mc, p, Some(cont), None, None, &env).map_err(EvalError::from)?;
        }

        return Ok(Value::Object(module_exports));
    }

    // Execute statements in module environment
    crate::core::evaluate_statements(mc, &env, &statements)?;

    // Log the exports stored in the provided `module_exports` object at trace level
    log::trace!("Module executed, exports keys:");
    for key in module_exports.borrow().properties.keys() {
        log::trace!(" - {}", key);
    }

    // Check if module.exports was reassigned (CommonJS style)
    if let Some(module_exports_val) = object_get_key_value(&module_obj, "exports") {
        match &*module_exports_val.borrow() {
            Value::Object(obj) if Gc::ptr_eq(*obj, module_exports) => {
                // exports was not reassigned, return the exports object
                Ok(Value::Object(module_exports))
            }
            other_value => {
                // exports was reassigned, return the new value
                Ok(other_value.clone())
            }
        }
    } else {
        // Fallback to exports object
        Ok(Value::Object(module_exports))
    }
}

pub fn import_from_module<'gc>(module_value: &Value<'gc>, specifier: &str) -> Result<Value<'gc>, JSError> {
    match module_value {
        Value::Object(obj) => match object_get_key_value(obj, specifier) {
            Some(val) => Ok(val.borrow().clone()),
            None => Err(crate::raise_eval_error!(format!("Export '{}' not found in module", specifier))),
        },
        _ => Err(crate::raise_eval_error!("Module is not an object")),
    }
}

pub(crate) fn get_or_create_module_cache<'gc>(
    mc: &MutationContext<'gc>,
    env: &JSObjectDataPtr<'gc>,
) -> Result<JSObjectDataPtr<'gc>, JSError> {
    if let Some(val_rc) = slot_get_chained(env, &InternalSlot::ModuleCache)
        && let Value::Object(obj) = &*val_rc.borrow()
    {
        return Ok(*obj);
    }

    let cache = new_js_object_data(mc);
    slot_set(mc, env, InternalSlot::ModuleCache, &Value::Object(cache));
    Ok(cache)
}

pub(crate) fn get_or_create_module_loading<'gc>(
    mc: &MutationContext<'gc>,
    env: &JSObjectDataPtr<'gc>,
) -> Result<JSObjectDataPtr<'gc>, JSError> {
    if let Some(val_rc) = slot_get_chained(env, &InternalSlot::ModuleLoading)
        && let Value::Object(obj) = &*val_rc.borrow()
    {
        return Ok(*obj);
    }

    let loading = new_js_object_data(mc);
    slot_set(mc, env, InternalSlot::ModuleLoading, &Value::Object(loading));
    Ok(loading)
}

fn get_or_create_module_eval_errors<'gc>(mc: &MutationContext<'gc>, env: &JSObjectDataPtr<'gc>) -> Result<JSObjectDataPtr<'gc>, JSError> {
    if let Some(val_rc) = slot_get_chained(env, &InternalSlot::ModuleEvalErrors)
        && let Value::Object(obj) = &*val_rc.borrow()
    {
        return Ok(*obj);
    }

    let errors = new_js_object_data(mc);
    slot_set(mc, env, InternalSlot::ModuleEvalErrors, &Value::Object(errors));
    Ok(errors)
}

fn get_or_create_module_async_pending<'gc>(mc: &MutationContext<'gc>, env: &JSObjectDataPtr<'gc>) -> Result<JSObjectDataPtr<'gc>, JSError> {
    if let Some(val_rc) = slot_get_chained(env, &InternalSlot::ModuleAsyncPending)
        && let Value::Object(obj) = &*val_rc.borrow()
    {
        return Ok(*obj);
    }

    let pending = new_js_object_data(mc);
    slot_set(mc, env, InternalSlot::ModuleAsyncPending, &Value::Object(pending));
    Ok(pending)
}

fn get_or_create_module_deferred_namespace_cache<'gc>(
    mc: &MutationContext<'gc>,
    env: &JSObjectDataPtr<'gc>,
) -> Result<JSObjectDataPtr<'gc>, JSError> {
    if let Some(val_rc) = slot_get_chained(env, &InternalSlot::ModuleDeferredNsCache)
        && let Value::Object(obj) = &*val_rc.borrow()
    {
        return Ok(*obj);
    }

    let cache = new_js_object_data(mc);
    slot_set(mc, env, InternalSlot::ModuleDeferredNsCache, &Value::Object(cache));
    Ok(cache)
}

fn get_or_create_module_namespace_cache<'gc>(
    mc: &MutationContext<'gc>,
    env: &JSObjectDataPtr<'gc>,
) -> Result<JSObjectDataPtr<'gc>, JSError> {
    if let Some(val_rc) = slot_get_chained(env, &InternalSlot::ModuleNamespaceCache)
        && let Value::Object(obj) = &*val_rc.borrow()
    {
        return Ok(*obj);
    }
    let cache = new_js_object_data(mc);
    slot_set(mc, env, InternalSlot::ModuleNamespaceCache, &Value::Object(cache));
    Ok(cache)
}

#[allow(dead_code)]
fn get_or_create_module_defer_pending_preloads<'gc>(
    mc: &MutationContext<'gc>,
    env: &JSObjectDataPtr<'gc>,
) -> Result<JSObjectDataPtr<'gc>, JSError> {
    if let Some(val_rc) = slot_get_chained(env, &InternalSlot::ModuleDeferPendingPreloads)
        && let Value::Object(obj) = &*val_rc.borrow()
    {
        return Ok(*obj);
    }

    let arr = crate::js_array::create_array(mc, env)?;
    slot_set(mc, env, InternalSlot::ModuleDeferPendingPreloads, &Value::Object(arr));
    Ok(arr)
}

#[allow(dead_code)]
pub fn queue_deferred_async_preload_module<'gc>(
    mc: &MutationContext<'gc>,
    env: &JSObjectDataPtr<'gc>,
    module_path: &str,
) -> Result<(), EvalError<'gc>> {
    let cache_env = resolve_cache_env(Some(*env)).unwrap_or(*env);
    let pending = get_or_create_module_defer_pending_preloads(mc, &cache_env).map_err(EvalError::from)?;

    let length = object_get_key_value(&pending, "length")
        .and_then(|v| match &*v.borrow() {
            Value::Number(n) => Some(*n as usize),
            _ => None,
        })
        .unwrap_or(0);

    for i in 0..length {
        if let Some(v) = object_get_key_value(&pending, i)
            && let Value::String(s) = v.borrow().clone()
            && crate::unicode::utf16_to_utf8(&s) == module_path
        {
            return Ok(());
        }
    }

    object_set_key_value(mc, &pending, length, &Value::String(crate::unicode::utf8_to_utf16(module_path))).map_err(EvalError::from)?;
    object_set_key_value(mc, &pending, "length", &Value::Number((length + 1) as f64)).map_err(EvalError::from)?;
    Ok(())
}

#[allow(dead_code)]
fn drain_microtasks<'gc>(mc: &MutationContext<'gc>) {
    for _ in 0..1024 {
        match crate::js_promise::run_event_loop(mc) {
            Ok(crate::js_promise::PollResult::Executed) => continue,
            Ok(crate::js_promise::PollResult::Wait(d)) => {
                std::thread::sleep(d);
                continue;
            }
            Ok(crate::js_promise::PollResult::Empty) => break,
            Err(_) => break,
        }
    }
}

#[allow(dead_code)]
pub fn flush_deferred_async_preload_modules<'gc>(mc: &MutationContext<'gc>, env: &JSObjectDataPtr<'gc>) -> Result<(), EvalError<'gc>> {
    let cache_env = resolve_cache_env(Some(*env)).unwrap_or(*env);
    let pending = get_or_create_module_defer_pending_preloads(mc, &cache_env).map_err(EvalError::from)?;
    let async_pending = get_or_create_module_async_pending(mc, &cache_env).map_err(EvalError::from)?;

    let length = object_get_key_value(&pending, "length")
        .and_then(|v| match &*v.borrow() {
            Value::Number(n) => Some(*n as usize),
            _ => None,
        })
        .unwrap_or(0);

    if length == 0 {
        let has_async_pending = async_pending
            .borrow()
            .properties
            .iter()
            .any(|(_, v)| matches!(*v.borrow(), Value::Boolean(true)));
        if has_async_pending {
            for _ in 0..8 {
                drain_microtasks(mc);
            }
        }
        return Ok(());
    }

    let mut modules = Vec::with_capacity(length);
    for i in 0..length {
        if let Some(v) = object_get_key_value(&pending, i)
            && let Value::String(s) = v.borrow().clone()
        {
            modules.push(crate::unicode::utf16_to_utf8(&s));
        }
    }
    object_set_key_value(mc, &pending, "length", &Value::Number(0.0)).map_err(EvalError::from)?;

    drain_microtasks(mc);
    let mut deferred_retry: Vec<String> = Vec::new();
    for module_path in &modules {
        let is_loading = is_module_loading_in_env_chain(&cache_env, module_path.as_str());
        let has_loading_dep = module_has_loading_dependency(module_path.as_str(), &cache_env, &mut std::collections::HashSet::new())
            .map_err(EvalError::from)?;
        let has_async_pending_dep =
            module_has_async_pending_dependency(module_path.as_str(), &cache_env, &mut std::collections::HashSet::new())
                .map_err(EvalError::from)?;
        if is_loading || has_loading_dep || has_async_pending_dep {
            deferred_retry.push(module_path.clone());
            continue;
        }
        preload_async_transitive_module(mc, module_path.as_str(), None, Some(cache_env))?;
    }
    for (idx, module_path) in deferred_retry.iter().enumerate() {
        object_set_key_value(mc, &pending, idx, &Value::String(crate::unicode::utf8_to_utf16(module_path))).map_err(EvalError::from)?;
    }
    object_set_key_value(mc, &pending, "length", &Value::Number(deferred_retry.len() as f64)).map_err(EvalError::from)?;
    for _ in 0..8 {
        drain_microtasks(mc);
    }

    if !deferred_retry.is_empty() {
        let mut still_retry: Vec<String> = Vec::new();
        for module_path in deferred_retry {
            let is_loading = is_module_loading_in_env_chain(&cache_env, module_path.as_str());
            let has_loading_dep = module_has_loading_dependency(module_path.as_str(), &cache_env, &mut std::collections::HashSet::new())
                .map_err(EvalError::from)?;
            if is_loading || has_loading_dep {
                still_retry.push(module_path);
                continue;
            }
            preload_async_transitive_module(mc, module_path.as_str(), None, Some(cache_env))?;
        }

        for (idx, module_path) in still_retry.iter().enumerate() {
            object_set_key_value(mc, &pending, idx, &Value::String(crate::unicode::utf8_to_utf16(module_path))).map_err(EvalError::from)?;
        }
        object_set_key_value(mc, &pending, "length", &Value::Number(still_retry.len() as f64)).map_err(EvalError::from)?;
    }

    Ok(())
}

fn get_symbol_to_string_tag<'gc>(env: &JSObjectDataPtr<'gc>) -> Option<Value<'gc>> {
    if let Some(sym_ctor_val) = crate::core::env_get(env, "Symbol")
        && let Value::Object(sym_ctor) = &*sym_ctor_val.borrow()
        && let Some(sym_tst) = object_get_key_value(sym_ctor, "toStringTag")
    {
        return Some(sym_tst.borrow().clone());
    }
    None
}

pub fn make_module_namespace_object<'gc>(
    mc: &MutationContext<'gc>,
    env: &JSObjectDataPtr<'gc>,
    exports_obj: &JSObjectDataPtr<'gc>,
) -> Result<Value<'gc>, EvalError<'gc>> {
    let cache_env = resolve_cache_env(Some(*env)).unwrap_or(*env);
    let ns_cache = get_or_create_module_namespace_cache(mc, &cache_env).map_err(EvalError::from)?;
    let cache_key = format!("ptr:{:p}", exports_obj.as_ptr());
    if let Some(cached_ns) = object_get_key_value(&ns_cache, cache_key.as_str()) {
        return Ok(cached_ns.borrow().clone());
    }

    let namespace_obj = new_js_object_data(mc);
    namespace_obj.borrow_mut(mc).prototype = None;

    let mut export_names: Vec<String> = exports_obj
        .borrow()
        .properties
        .keys()
        .filter_map(|key| match key {
            crate::core::PropertyKey::String(s) => Some(s.clone()),
            _ => None,
        })
        .collect();

    let module_path_for_exports = {
        let mut found: Option<String> = None;
        if let Some(cache_val) = slot_get_chained(&cache_env, &InternalSlot::ModuleCache)
            && let Value::Object(cache_obj) = cache_val.borrow().clone()
        {
            for (k, v) in &cache_obj.borrow().properties {
                if let crate::core::PropertyKey::String(path) = k
                    && let Value::Object(cached_exports) = v.borrow().clone()
                    && crate::core::Gc::ptr_eq(cached_exports, *exports_obj)
                {
                    found = Some(path.clone());
                    break;
                }
            }
        }

        if found.is_none()
            && let Some(exports_val) = object_get_key_value(env, "exports")
            && let Value::Object(self_exports) = exports_val.borrow().clone()
            && crate::core::Gc::ptr_eq(self_exports, *exports_obj)
            && let Some(path_val) = slot_get_chained(env, &InternalSlot::Filepath)
            && let Value::String(path16) = path_val.borrow().clone()
        {
            found = Some(crate::unicode::utf16_to_utf8(&path16));
        }

        found
    };

    if let Some(module_path) = module_path_for_exports
        && !module_path.ends_with(".json")
    {
        let mut names = std::collections::BTreeSet::new();
        let mut visited = std::collections::HashSet::new();
        if collect_module_export_names_recursive(module_path.as_str(), &mut names, &mut visited).is_ok() {
            export_names = names.into_iter().collect();
        }
    }

    export_names.sort();
    export_names.dedup();

    let export_names_meta = new_js_object_data(mc);
    for name in &export_names {
        object_set_key_value(mc, &export_names_meta, name.as_str(), &Value::Boolean(true)).map_err(EvalError::from)?;
    }
    object_set_key_value(
        mc,
        &namespace_obj,
        crate::core::PropertyKey::Private("__ns_export_names".to_string(), 1),
        &Value::Object(export_names_meta),
    )
    .map_err(EvalError::from)?;

    for name in export_names {
        let key = crate::core::PropertyKey::String(name.clone());
        if let Some(mut pd) = crate::core::build_property_descriptor(mc, exports_obj, &key) {
            pd.enumerable = Some(true);
            pd.configurable = Some(false);
            if pd.value.is_some() && pd.writable.is_none() {
                pd.writable = Some(true);
            }
            let desc = pd.to_object(mc).map_err(EvalError::from)?;
            crate::js_object::define_property_internal(mc, &namespace_obj, key, &desc).map_err(EvalError::from)?;
        } else {
            let hidden = format!("__ns_src_{}_{}", cache_key.replace(':', "_"), name);
            crate::core::env_set(mc, &cache_env, hidden.as_str(), &Value::Object(*exports_obj)).map_err(EvalError::from)?;

            let getter_body = vec![Statement {
                kind: Box::new(StatementKind::Return(Some(Expr::Property(
                    Box::new(Expr::Var(hidden, None, None)),
                    name.clone(),
                )))),
                line: 0,
                column: 0,
            }];

            let getter_val = Value::Getter(getter_body, cache_env, None);
            let prop = Value::Property {
                value: None,
                getter: Some(Box::new(getter_val)),
                setter: None,
            };

            object_set_key_value(mc, &namespace_obj, name.as_str(), &prop).map_err(EvalError::from)?;
            namespace_obj
                .borrow_mut(mc)
                .set_non_configurable(crate::core::PropertyKey::String(name));
        }
    }

    if let Some(sym_tst_val) = get_symbol_to_string_tag(env)
        && let Value::Symbol(sym_tst) = sym_tst_val
    {
        let desc = create_descriptor_object(mc, &Value::String(crate::unicode::utf8_to_utf16("Module")), false, false, false)
            .map_err(EvalError::from)?;
        crate::js_object::define_property_internal(mc, &namespace_obj, crate::core::PropertyKey::Symbol(sym_tst), &desc)
            .map_err(EvalError::from)?;
    }

    namespace_obj.borrow_mut(mc).prevent_extensions();
    let namespace_val = Value::Object(namespace_obj);
    object_set_key_value(mc, &ns_cache, cache_key.as_str(), &namespace_val).map_err(EvalError::from)?;
    Ok(namespace_val)
}

fn collect_module_export_names(stmts: &[crate::core::Statement], out: &mut std::collections::BTreeSet<String>) {
    for stmt in stmts {
        if let StatementKind::Export(specifiers, inner_stmt, source) = &*stmt.kind {
            for spec in specifiers {
                match spec {
                    ExportSpecifier::Named(name, alias) => {
                        out.insert(alias.as_ref().unwrap_or(name).clone());
                    }
                    ExportSpecifier::Namespace(name) => {
                        out.insert(name.clone());
                    }
                    ExportSpecifier::Default(_) => {
                        out.insert("default".to_string());
                    }
                    ExportSpecifier::Star => {}
                }
            }

            if source.is_none()
                && let Some(inner) = inner_stmt
            {
                match &*inner.kind {
                    StatementKind::Let(decls) | StatementKind::Var(decls) => {
                        for (name, _) in decls {
                            out.insert(name.clone());
                        }
                    }
                    StatementKind::Const(decls) => {
                        for (name, _) in decls {
                            out.insert(name.clone());
                        }
                    }
                    StatementKind::FunctionDeclaration(name, ..) => {
                        out.insert(name.clone());
                    }
                    StatementKind::Class(class_def) => {
                        out.insert(class_def.name.clone());
                    }
                    _ => {}
                }
            }
        }
    }
}

fn collect_module_export_names_recursive(
    module_path: &str,
    out: &mut std::collections::BTreeSet<String>,
    visited: &mut std::collections::HashSet<String>,
) -> Result<(), JSError> {
    if !visited.insert(module_path.to_string()) {
        return Ok(());
    }

    let content = std::fs::read_to_string(module_path)
        .map_err(|e| crate::raise_eval_error!(format!("Failed to read module '{}': {e}", module_path)))?;
    let tokens = crate::core::tokenize(&content)?;
    let mut index = 0;
    crate::core::push_await_context();
    let parse_result = crate::core::parse_statements(&tokens, &mut index);
    crate::core::pop_await_context();
    let statements = parse_result?;

    let mut direct_names = std::collections::BTreeSet::new();
    collect_module_export_names(&statements, &mut direct_names);

    let mut star_name_counts: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    let mut star_name_providers: std::collections::HashMap<String, std::collections::BTreeSet<String>> = std::collections::HashMap::new();

    for stmt in &statements {
        if let StatementKind::Export(specifiers, _, Some(source_name)) = &*stmt.kind
            && specifiers.iter().any(|s| matches!(s, ExportSpecifier::Star))
        {
            let resolved = resolve_module_path(source_name, Some(module_path))?;
            let mut child_names = std::collections::BTreeSet::new();
            collect_module_export_names_recursive(&resolved, &mut child_names, visited)?;
            for name in child_names {
                if name != "default" {
                    *star_name_counts.entry(name.clone()).or_insert(0) += 1;
                    star_name_providers.entry(name).or_default().insert(resolved.clone());
                }
            }
        }
    }

    for (name, count) in star_name_counts {
        let is_unambiguous_duplicate_indirect = if count > 1 && !direct_names.contains(&name) {
            if let Some(providers) = star_name_providers.get(&name) {
                providers.iter().all(|provider| {
                    module_has_local_export_name(provider.as_str(), name.as_str())
                        .map(|is_local| !is_local)
                        .unwrap_or(false)
                })
            } else {
                false
            }
        } else {
            false
        };

        if count == 1 || direct_names.contains(&name) || is_unambiguous_duplicate_indirect {
            direct_names.insert(name);
        }
    }

    out.extend(direct_names);

    Ok(())
}

fn module_has_local_export_name(module_path: &str, export_name: &str) -> Result<bool, JSError> {
    let content = std::fs::read_to_string(module_path)
        .map_err(|e| crate::raise_eval_error!(format!("Failed to read module '{}': {e}", module_path)))?;
    let tokens = crate::core::tokenize(&content)?;
    let mut index = 0;
    crate::core::push_await_context();
    let parse_result = crate::core::parse_statements(&tokens, &mut index);
    crate::core::pop_await_context();
    let statements = parse_result?;

    let mut imported_locals = std::collections::HashSet::new();
    for stmt in &statements {
        if let StatementKind::Import(specifiers, _) = &*stmt.kind {
            for spec in specifiers {
                match spec {
                    crate::core::ImportSpecifier::Named(name, alias) => {
                        imported_locals.insert(alias.as_ref().unwrap_or(name).clone());
                    }
                    crate::core::ImportSpecifier::Default(name) | crate::core::ImportSpecifier::Namespace(name) => {
                        imported_locals.insert(name.clone());
                    }
                }
            }
        }
    }

    for stmt in &statements {
        if let StatementKind::Export(specifiers, inner_stmt, source) = &*stmt.kind
            && source.is_none()
        {
            if let Some(inner) = inner_stmt {
                match &*inner.kind {
                    StatementKind::Var(decls) | StatementKind::Let(decls) => {
                        if decls.iter().any(|(name, _)| name == export_name) {
                            return Ok(true);
                        }
                    }
                    StatementKind::Const(decls) => {
                        if decls.iter().any(|(name, _)| name == export_name) {
                            return Ok(true);
                        }
                    }
                    StatementKind::FunctionDeclaration(name, ..) => {
                        if name == export_name {
                            return Ok(true);
                        }
                    }
                    StatementKind::Class(class_def) => {
                        if class_def.name == export_name {
                            return Ok(true);
                        }
                    }
                    _ => {}
                }
            }

            for spec in specifiers {
                if let ExportSpecifier::Named(local_name, alias) = spec {
                    let out_name = alias.as_ref().unwrap_or(local_name);
                    if out_name == export_name {
                        return Ok(!imported_locals.contains(local_name));
                    }
                }
            }
        }
    }

    Ok(false)
}

pub fn load_module_deferred_namespace<'gc>(
    mc: &MutationContext<'gc>,
    module_name: &str,
    base_path: Option<&str>,
    caller_env: Option<JSObjectDataPtr<'gc>>,
) -> Result<Value<'gc>, EvalError<'gc>> {
    if matches!(module_name, "math" | "console" | "std" | "os") {
        return load_module(mc, module_name, base_path, caller_env);
    }

    let module_path = resolve_module_path(module_name, base_path).map_err(EvalError::from)?;

    let cache_env = resolve_cache_env(caller_env).unwrap_or_else(|| new_js_object_data(mc));
    let deferred_cache = get_or_create_module_deferred_namespace_cache(mc, &cache_env).map_err(EvalError::from)?;

    if let Some(ns_val_rc) = object_get_key_value(&deferred_cache, module_path.as_str()) {
        return Ok(ns_val_rc.borrow().clone());
    }

    let namespace_obj = new_js_object_data(mc);
    namespace_obj.borrow_mut(mc).prototype = None;
    namespace_obj.borrow_mut(mc).deferred_module_path = Some(module_path.clone());
    namespace_obj.borrow_mut(mc).deferred_cache_env = Some(cache_env);

    let ns_value = Value::Object(namespace_obj);
    object_set_key_value(mc, &deferred_cache, module_path.as_str(), &ns_value).map_err(EvalError::from)?;

    let module_cache = get_or_create_module_cache(mc, &cache_env).map_err(EvalError::from)?;
    let mut export_names: Vec<String> = Vec::new();
    let mut export_values: std::collections::HashMap<String, Value<'gc>> = std::collections::HashMap::new();

    if let Some(cached_val) = object_get_key_value(&module_cache, module_path.as_str())
        && let Value::Object(exports_obj) = cached_val.borrow().clone()
    {
        for key in exports_obj.borrow().properties.keys() {
            if let crate::core::PropertyKey::String(s) = key
                && let Some(v) = object_get_key_value(&exports_obj, s)
            {
                export_values.insert(s.clone(), v.borrow().clone());
            }
        }

        let mut names = std::collections::BTreeSet::new();
        let mut visited = std::collections::HashSet::new();
        collect_module_export_names_recursive(module_path.as_str(), &mut names, &mut visited).map_err(EvalError::from)?;
        export_names.extend(names);
    } else {
        let mut names = std::collections::BTreeSet::new();
        let mut visited = std::collections::HashSet::new();
        collect_module_export_names_recursive(module_path.as_str(), &mut names, &mut visited).map_err(EvalError::from)?;
        export_names.extend(names);
    }

    export_names.sort();
    export_names.dedup();

    for name in export_names {
        let value = export_values.remove(&name).unwrap_or(Value::Undefined);
        object_set_key_value(mc, &namespace_obj, name.as_str(), &value).map_err(EvalError::from)?;
        namespace_obj.borrow_mut(mc).set_non_configurable(name.clone());
    }

    if let Some(sym_tst_val) = get_symbol_to_string_tag(&cache_env)
        && let Value::Symbol(sym_tst) = sym_tst_val
    {
        let desc = create_descriptor_object(
            mc,
            &Value::String(crate::unicode::utf8_to_utf16("Deferred Module")),
            false,
            false,
            false,
        )
        .map_err(EvalError::from)?;
        crate::js_object::define_property_internal(mc, &namespace_obj, crate::core::PropertyKey::Symbol(sym_tst), &desc)
            .map_err(EvalError::from)?;
    }

    namespace_obj.borrow_mut(mc).prevent_extensions();

    Ok(Value::Object(namespace_obj))
}

pub fn ensure_deferred_namespace_evaluated<'gc>(
    mc: &MutationContext<'gc>,
    env: &JSObjectDataPtr<'gc>,
    obj: &JSObjectDataPtr<'gc>,
    key_hint: Option<&str>,
) -> Result<bool, EvalError<'gc>> {
    if matches!(key_hint, Some("then")) {
        return Ok(false);
    }

    fn scan_cache_for_obj<'gc>(holder: &JSObjectDataPtr<'gc>, obj: &JSObjectDataPtr<'gc>) -> Option<String> {
        if let Some(cache_val) = slot_get_chained(holder, &InternalSlot::ModuleDeferredNsCache)
            && let Value::Object(cache_obj) = cache_val.borrow().clone()
        {
            for (k, v) in &cache_obj.borrow().properties {
                if let crate::core::PropertyKey::String(path) = k
                    && let Value::Object(ns_obj) = v.borrow().clone()
                    && crate::core::Gc::ptr_eq(ns_obj, *obj)
                {
                    return Some(path.clone());
                }
            }
        }
        None
    }

    let mut found_path: Option<String> = obj.borrow().deferred_module_path.clone();
    let preferred_cache_env: Option<JSObjectDataPtr<'gc>> = obj.borrow().deferred_cache_env;

    let mut cur = Some(*env);
    if found_path.is_none() {
        while let Some(e) = cur {
            found_path = scan_cache_for_obj(&e, obj);
            if found_path.is_none()
                && let Some(global_val) = object_get_key_value(&e, "globalThis")
                && let Value::Object(global_obj) = global_val.borrow().clone()
            {
                found_path = scan_cache_for_obj(&global_obj, obj);
            }
            if found_path.is_some() {
                break;
            }
            cur = e.borrow().prototype;
        }
    }

    if found_path.is_none() {
        let mut cur = Some(*env);
        while let Some(e) = cur {
            let mut object_values: Vec<JSObjectDataPtr<'gc>> = Vec::new();
            {
                let b = e.borrow();
                for v in b.properties.values() {
                    if let Value::Object(o) = v.borrow().clone() {
                        object_values.push(o);
                    }
                }
            }

            for holder in object_values {
                found_path = scan_cache_for_obj(&holder, obj);
                if found_path.is_some() {
                    break;
                }
            }

            if found_path.is_some() {
                break;
            }
            cur = e.borrow().prototype;
        }
    }

    let Some(module_path) = found_path else {
        return Ok(false);
    };

    let cache_env = preferred_cache_env.unwrap_or(*env);

    // import-defer: if the target module is currently evaluating/loading,
    // accessing namespace exports should throw TypeError rather than attempting
    // a recursive sync load/eval.
    let module_is_loading = is_module_loading_in_env_chain(&cache_env, module_path.as_str());
    let module_is_async_pending = get_or_create_module_async_pending(mc, &cache_env)
        .ok()
        .and_then(|pending| object_get_key_value(&pending, module_path.as_str()))
        .map(|v| matches!(*v.borrow(), Value::Boolean(true)))
        .unwrap_or(false);
    let has_loading_dependency =
        module_has_loading_dependency(module_path.as_str(), &cache_env, &mut std::collections::HashSet::new()).map_err(EvalError::from)?;

    let mut same_module_context = false;
    let mut cur_file_env = Some(*env);
    while let Some(e) = cur_file_env {
        if let Some(cur_file_val) = slot_get_chained(&e, &InternalSlot::Filepath)
            && let Value::String(cur_file) = cur_file_val.borrow().clone()
            && crate::unicode::utf16_to_utf8(&cur_file) == module_path
        {
            same_module_context = true;
            break;
        }
        cur_file_env = e.borrow().prototype;
    }

    if module_is_async_pending && same_module_context {
        if let Ok(async_pending) = get_or_create_module_async_pending(mc, &cache_env) {
            let _ = object_set_key_value(mc, &async_pending, module_path.as_str(), &Value::Boolean(false));
        }
        return Err(crate::raise_type_error!("Module is currently evaluating").into());
    }

    if module_is_loading || has_loading_dependency {
        return Err(crate::raise_type_error!("Module is currently evaluating").into());
    }

    let _ = module_requested_modules(module_path.as_str());

    let _ = load_module(mc, module_path.as_str(), None, Some(cache_env))?;

    Ok(true)
}

fn resolve_cache_env<'gc>(caller_env: Option<JSObjectDataPtr<'gc>>) -> Option<JSObjectDataPtr<'gc>> {
    if let Some(env) = caller_env {
        if let Some(global_val) = crate::core::env_get(&env, "globalThis")
            && let Value::Object(global_obj) = global_val.borrow().clone()
        {
            return Some(global_obj);
        }
        return Some(env);
    }
    None
}

#[allow(dead_code)]
pub fn get_module_default_export<'gc>(module_value: &Value<'gc>) -> Value<'gc> {
    match module_value {
        Value::Object(_) => {
            // For object modules, try to get default export, otherwise return the module itself
            match import_from_module(module_value, "default") {
                Ok(default_value) => default_value,
                Err(_) => module_value.clone(),
            }
        }
        _ => {
            // For non-object modules (like functions), the module value itself is the default export
            module_value.clone()
        }
    }
}
