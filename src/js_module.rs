use crate::{
    JSError, Value,
    core::{obj_get_value, obj_set_value},
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

        obj_set_value(&module_exports, &"PI".into(), pi)?;
        obj_set_value(&module_exports, &"E".into(), e)?;

        // Add a simple function (just return the input for now)
        let identity_func = Value::Closure(
            vec!["x".to_string()],
            vec![crate::core::Statement::Return(Some(crate::core::Expr::Var("x".to_string())))],
            module_exports.clone(),
        );
        obj_set_value(&module_exports, &"identity".into(), identity_func)?;
    } else if module_name == "console" {
        // Create console module with log function
        // Create a function that directly handles console.log calls
        let log_func = Value::Function("console.log".to_string());
        obj_set_value(&module_exports, &"log".into(), log_func)?;
    } else {
        // Try to load as a file
        match load_module_from_file(module_name, base_path) {
            Ok(loaded_module) => return Ok(loaded_module),
            Err(_) => {
                // Default empty module if file loading fails
                log::debug!("Failed to load module '{}' from file, returning empty module", module_name);
            }
        }
    }

    Ok(Value::Object(module_exports))
}

fn load_module_from_file(module_name: &str, base_path: Option<&str>) -> Result<Value, JSError> {
    // Resolve the module path
    let module_path = resolve_module_path(module_name, base_path)?;

    // Read the file
    let content = std::fs::read_to_string(&module_path)
        .map_err(|e| crate::raise_eval_error!(format!("Failed to read module file '{}': {}", module_path, e)))?;

    // Create module exports object
    let module_exports = Rc::new(RefCell::new(crate::core::JSObjectData::new()));

    // Execute the module in a special environment that captures exports
    execute_module(&content, &module_exports, &module_path)?;

    Ok(Value::Object(module_exports))
}

fn resolve_module_path(module_name: &str, base_path: Option<&str>) -> Result<String, JSError> {
    let path = Path::new(module_name);

    // If it's an absolute path or starts with ./ or ../, treat as file path
    if path.is_absolute() || module_name.starts_with("./") || module_name.starts_with("../") {
        let mut full_path = if let Some(base) = base_path {
            Path::new(base).parent().unwrap_or(Path::new(".")).join(module_name)
        } else {
            Path::new(module_name).to_path_buf()
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

fn execute_module(content: &str, module_exports: &Rc<RefCell<crate::core::JSObjectData>>, _module_path: &str) -> Result<(), JSError> {
    // Create a module environment
    let env = Rc::new(RefCell::new(crate::core::JSObjectData::new()));
    env.borrow_mut().is_function_scope = true;

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
        Rc::new(RefCell::new(Value::Object(module_obj))),
    );

    // Initialize global constructors
    crate::core::initialize_global_constructors(&env)?;

    // Parse and execute the module content
    let mut tokens = crate::core::tokenize(content)?;
    let statements = crate::core::parse_statements(&mut tokens)?;

    // Execute statements in module environment
    crate::core::evaluate_statements(&env, &statements)?;

    Ok(())
}

pub fn import_from_module(module_value: &Value, specifier: &str) -> Result<Value, JSError> {
    match module_value {
        Value::Object(obj) => match obj_get_value(obj, &specifier.into())? {
            Some(val) => Ok(val.borrow().clone()),
            None => Err(crate::raise_eval_error!(format!("Export '{}' not found in module", specifier))),
        },
        _ => Err(crate::raise_eval_error!("Module is not an object")),
    }
}
