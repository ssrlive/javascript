use crate::error::JSError;
use crate::quickjs::{evaluate_expr, obj_set_val, Expr, JSObjectData, Value};

/// Create the console object with logging functions
pub fn make_console_object() -> JSObjectData {
    let mut console_obj = JSObjectData::new();
    obj_set_val(&mut console_obj, "log", Value::Function("console.log".to_string()));
    console_obj
}

/// Handle console object method calls
pub fn handle_console_method(method: &str, args: &[Expr], env: &JSObjectData) -> Result<Value, JSError> {
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
