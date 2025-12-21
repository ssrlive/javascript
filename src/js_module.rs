use crate::{
    JSError, Value,
    core::{ClosureData, DestructuringElement, Expr, Statement, StatementKind, obj_get_key_value, obj_set_key_value},
};
use std::cell::RefCell;
use std::path::Path;
use std::rc::Rc;

pub fn load_module(module_name: &str, base_path: Option<&str>) -> Result<Value, JSError> {
    // Create a new object for the module
    let module_exports = Rc::new(RefCell::new(crate::core::JSObjectData::new()));

    // For demonstration, create a simple module with some exports
    if module_name == "math" {
        // Simulate loading a math module
        let pi = Value::Number(std::f64::consts::PI);
        let e = Value::Number(std::f64::consts::E);

        obj_set_key_value(&module_exports, &"PI".into(), pi)?;
        obj_set_key_value(&module_exports, &"E".into(), e)?;

        // Add a simple function (just return the input for now)
        let identity_func = Value::Closure(Rc::new(ClosureData::new(
            &[DestructuringElement::Variable("x".to_string(), None)],
            &[Statement {
                kind: StatementKind::Return(Some(Expr::Var("x".to_string(), None, None))),
                line: 0,
                column: 0,
            }],
            &module_exports,
            None,
        )));
        obj_set_key_value(&module_exports, &"identity".into(), identity_func)?;
    } else if module_name == "console" {
        // Create console module with log function
        // Create a function that directly handles console.log calls
        let log_func = Value::Function("console.log".to_string());
        obj_set_key_value(&module_exports, &"log".into(), log_func)?;
    } else if module_name == "std" {
        let std_obj = crate::js_std::make_std_object()?;
        return Ok(Value::Object(std_obj));
    } else if module_name == "os" {
        let os_obj = crate::js_os::make_os_object()?;
        return Ok(Value::Object(os_obj));
    } else {
        // Try to load as a file
        match load_module_from_file(module_name, base_path) {
            Ok(loaded_module) => return Ok(loaded_module),
            Err(_) => {
                // Default empty module if file loading fails
                log::debug!("Failed to load module '{module_name}' from file, returning empty module");
            }
        }
    }

    Ok(Value::Object(module_exports))
}

fn load_module_from_file(module_name: &str, base_path: Option<&str>) -> Result<Value, JSError> {
    // Resolve the module path
    let module_path = resolve_module_path(module_name, base_path)?;

    // Read the file
    let content = crate::core::read_script_file(&module_path)?;

    // Execute the module and get the final module value
    execute_module(&content, &module_path)
}

fn resolve_module_path(module_name: &str, base_path: Option<&str>) -> Result<String, JSError> {
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
                .map_err(|e| raise_eval_error!(format!("Failed to get current directory: {e}")))?
                .join(module_name)
        };

        // Add .js extension if not present
        if full_path.extension().is_none() {
            full_path.set_extension("js");
        }

        // Canonicalize the path
        match full_path.canonicalize() {
            Ok(canonical) => Ok(canonical.to_string_lossy().to_string()),
            Err(_) => Err(raise_eval_error!(format!("Module file not found: {}", full_path.display()))),
        }
    } else {
        // For now, treat relative paths as relative to current directory
        let mut full_path = Path::new(module_name).to_path_buf();
        if full_path.extension().is_none() {
            full_path.set_extension("js");
        }

        match full_path.canonicalize() {
            Ok(canonical) => Ok(canonical.to_string_lossy().to_string()),
            Err(_) => Err(raise_eval_error!(format!("Module file not found: {}", full_path.display()))),
        }
    }
}

fn execute_module(content: &str, module_path: &str) -> Result<Value, JSError> {
    // Create module exports object
    let module_exports = Rc::new(RefCell::new(crate::core::JSObjectData::new()));

    // Create a module environment
    let env = Rc::new(RefCell::new(crate::core::JSObjectData::new()));
    env.borrow_mut().is_function_scope = true;

    // Record a module path on the module environment so stack frames / errors can include it
    // Store as `__script_name` similarly to `evaluate_script`.
    let val = Value::String(crate::unicode::utf8_to_utf16(module_path));
    obj_set_key_value(&env, &"__script_name".into(), val)?;

    // Add exports object to the environment
    env.borrow_mut().insert(
        crate::core::PropertyKey::String("exports".to_string()),
        Rc::new(RefCell::new(Value::Object(module_exports.clone()))),
    );

    // Add module object with exports
    let module_obj = Rc::new(RefCell::new(crate::core::JSObjectData::new()));
    module_obj.borrow_mut().insert(
        crate::core::PropertyKey::String("exports".to_string()),
        Rc::new(RefCell::new(Value::Object(module_exports.clone()))),
    );
    env.borrow_mut().insert(
        crate::core::PropertyKey::String("module".to_string()),
        Rc::new(RefCell::new(Value::Object(module_obj.clone()))),
    );

    // Initialize global constructors
    crate::core::initialize_global_constructors(&env)?;

    // Expose `globalThis` binding in module environment as well
    crate::core::obj_set_key_value(&env, &"globalThis".into(), crate::core::Value::Object(env.clone()))?;

    // Parse and execute the module content
    let mut tokens = crate::core::tokenize(content)?;
    let statements = crate::core::parse_statements(&mut tokens)?;

    // Execute statements in module environment
    crate::core::evaluate_statements(&env, &statements)?;

    // Log the exports stored in the provided `module_exports` object at trace level
    log::trace!("Module executed, exports keys:");
    for key in module_exports.borrow().keys() {
        log::trace!(" - {}", key);
    }

    // Check if module.exports was reassigned (CommonJS style)
    if let Some(module_exports_val) = obj_get_key_value(&module_obj, &"exports".into())? {
        match &*module_exports_val.borrow() {
            Value::Object(obj) if Rc::ptr_eq(obj, &module_exports) => {
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

pub fn import_from_module(module_value: &Value, specifier: &str) -> Result<Value, JSError> {
    match module_value {
        Value::Object(obj) => match obj_get_key_value(obj, &specifier.into())? {
            Some(val) => Ok(val.borrow().clone()),
            None => Err(raise_eval_error!(format!("Export '{}' not found in module", specifier))),
        },
        _ => Err(raise_eval_error!("Module is not an object")),
    }
}

#[allow(dead_code)]
pub fn get_module_default_export(module_value: &Value) -> Value {
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
