use crate::error::JSError;
use crate::quickjs::{evaluate_expr, obj_set_value, Expr, JSObjectData, JSObjectDataPtr, Value};
use std::cell::RefCell;
use std::rc::Rc;

/// Create the console object with logging functions
pub fn make_console_object() -> Result<JSObjectDataPtr, JSError> {
    let console_obj = Rc::new(RefCell::new(JSObjectData::new()));
    obj_set_value(&console_obj, "log", Value::Function("console.log".to_string()))?;
    Ok(console_obj)
}

/// Handle console object method calls
pub fn handle_console_method(method: &str, args: &[Expr], env: &JSObjectDataPtr) -> Result<Value, JSError> {
    match method {
        "log" => {
            // console.log call
            for arg in args {
                let arg_val = evaluate_expr(env, arg)?;
                match arg_val {
                    Value::Number(n) => print!("{}", n),
                    Value::String(s) => {
                        print!("{}", String::from_utf16_lossy(&s))
                    }
                    Value::Boolean(b) => print!("{}", b),
                    Value::Undefined => print!("undefined"),
                    Value::Object(_) => print!("[object Object]"),
                    Value::Function(name) => print!("[Function: {}]", name),
                    Value::Closure(_, _, _) => print!("[Function]"),
                    Value::ClassDefinition(_) => print!("[Class]"),
                    Value::Getter(_, _) => print!("[Getter]"),
                    Value::Setter(_, _, _) => print!("[Setter]"),
                    Value::Property { .. } => print!("[Property]"),
                }
            }
            println!();
            Ok(Value::Undefined)
        }
        _ => Err(JSError::EvaluationError {
            message: format!("Console method {method} not implemented"),
        }),
    }
}
