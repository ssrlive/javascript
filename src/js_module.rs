use crate::{
    JSError, Value,
    core::{
        ClosureData, DestructuringElement, EvalError, Expr, JSObjectDataPtr, Statement, StatementKind, object_get_key_value,
        object_set_key_value,
    },
    core::{Gc, MutationContext, new_gc_cell_ptr},
    new_js_object_data,
};
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

        object_set_key_value(mc, &module_exports, "PI", pi)?;
        object_set_key_value(mc, &module_exports, "E", e)?;

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
        object_set_key_value(mc, &module_exports, "identity", identity_func.clone())?;
        object_set_key_value(mc, &module_exports, "default", identity_func)?;
    } else if module_name == "console" {
        // Create console module with log function
        // Create a function that directly handles console.log calls
        let log_func = Value::Function("console.log".to_string());
        object_set_key_value(mc, &module_exports, "log", log_func)?;
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

fn load_module_from_file<'gc>(
    mc: &MutationContext<'gc>,
    module_name: &str,
    base_path: Option<&str>,
    caller_env: Option<JSObjectDataPtr<'gc>>,
) -> Result<Value<'gc>, EvalError<'gc>> {
    // Resolve the module path
    let module_path = resolve_module_path(module_name, base_path).map_err(EvalError::from)?;

    let cache_env = resolve_cache_env(caller_env);
    if let Some(cache_env) = cache_env {
        let cache = get_or_create_module_cache(mc, &cache_env)?;
        if let Some(val_rc) = object_get_key_value(&cache, module_path.as_str()) {
            return Ok(val_rc.borrow().clone());
        }

        let loading = get_or_create_module_loading(mc, &cache_env)?;
        if let Some(flag_rc) = object_get_key_value(&loading, module_path.as_str())
            && matches!(*flag_rc.borrow(), Value::Boolean(true))
        {
            return Err(crate::raise_syntax_error!("Circular module import").into());
        }

        object_set_key_value(mc, &loading, module_path.as_str(), Value::Boolean(true))?;

        let module_exports = new_js_object_data(mc);
        object_set_key_value(mc, &cache, module_path.as_str(), Value::Object(module_exports))?;

        // Read the file
        let content = crate::core::read_script_file(&module_path).map_err(EvalError::from)?;

        // Execute the module and get the final module value
        let value = execute_module(mc, &content, &module_path, caller_env, Some(module_exports))?;

        object_set_key_value(mc, &cache, module_path.as_str(), value.clone())?;
        object_set_key_value(mc, &loading, module_path.as_str(), Value::Boolean(false))?;
        return Ok(value);
    }

    // Read the file
    let content = crate::core::read_script_file(&module_path).map_err(EvalError::from)?;

    // Execute the module and get the final module value
    execute_module(mc, &content, &module_path, caller_env, None)
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

fn execute_module<'gc>(
    mc: &MutationContext<'gc>,
    content: &str,
    module_path: &str,
    caller_env: Option<JSObjectDataPtr<'gc>>,
    module_exports_override: Option<JSObjectDataPtr<'gc>>,
) -> Result<Value<'gc>, EvalError<'gc>> {
    // Create module exports object
    let module_exports = module_exports_override.unwrap_or_else(|| new_js_object_data(mc));

    // Create a module environment
    let env = new_js_object_data(mc);
    env.borrow_mut(mc).is_function_scope = true;

    if let Some(caller) = caller_env {
        env.borrow_mut(mc).prototype = Some(caller);
    }

    // Record a module path on the module environment so stack frames / errors can include it
    // Store as `__filepath` similarly to `evaluate_script`.
    let val = Value::String(crate::unicode::utf8_to_utf16(module_path));
    object_set_key_value(mc, &env, "__filepath", val)?;

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

    if caller_env.is_none() {
        // Initialize global constructors for standalone module execution
        crate::core::initialize_global_constructors(mc, &env)?;
        object_set_key_value(mc, &env, "globalThis", crate::core::Value::Object(env))?;
    } else if let Some(caller) = caller_env {
        let global_obj = if let Some(global_val) = object_get_key_value(&caller, "globalThis") {
            match global_val.borrow().clone() {
                Value::Object(global_obj) => global_obj,
                _ => caller,
            }
        } else {
            caller
        };
        object_set_key_value(mc, &env, "globalThis", crate::core::Value::Object(global_obj))?;
    }

    // Parse and execute the module content
    let tokens = crate::core::tokenize(content).map_err(EvalError::from)?;
    let mut index = 0;
    let statements = crate::core::parse_statements(&tokens, &mut index).map_err(EvalError::from)?;

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
    if let Some(val_rc) = object_get_key_value(env, "__module_cache")
        && let Value::Object(obj) = &*val_rc.borrow()
    {
        return Ok(*obj);
    }

    let cache = new_js_object_data(mc);
    object_set_key_value(mc, env, "__module_cache", Value::Object(cache))?;
    Ok(cache)
}

pub(crate) fn get_or_create_module_loading<'gc>(
    mc: &MutationContext<'gc>,
    env: &JSObjectDataPtr<'gc>,
) -> Result<JSObjectDataPtr<'gc>, JSError> {
    if let Some(val_rc) = object_get_key_value(env, "__module_loading")
        && let Value::Object(obj) = &*val_rc.borrow()
    {
        return Ok(*obj);
    }

    let loading = new_js_object_data(mc);
    object_set_key_value(mc, env, "__module_loading", Value::Object(loading))?;
    Ok(loading)
}

fn resolve_cache_env<'gc>(caller_env: Option<JSObjectDataPtr<'gc>>) -> Option<JSObjectDataPtr<'gc>> {
    if let Some(env) = caller_env {
        if let Some(global_val) = object_get_key_value(&env, "globalThis")
            && let Value::Object(global_obj) = &*global_val.borrow()
        {
            return Some(*global_obj);
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
