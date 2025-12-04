use crate::core::{Expr, JSObjectData, JSObjectDataPtr, Value, evaluate_expr, evaluate_statements, obj_set_value};
use crate::error::JSError;
use crate::eval_error_here;
use std::cell::RefCell;
use std::rc::Rc;

/// Create the assert object with testing functions
pub fn make_assert_object() -> Result<JSObjectDataPtr, JSError> {
    let assert_obj = Rc::new(RefCell::new(JSObjectData::new()));
    obj_set_value(&assert_obj, &"sameValue".into(), Value::Function("assert.sameValue".to_string()))?;
    Ok(assert_obj)
}

/// Handle assert object method calls
pub fn handle_assert_method(method: &str, args: &[Expr], env: &JSObjectDataPtr) -> Result<Value, JSError> {
    match method {
        "sameValue" => {
            if args.len() < 2 || args.len() > 3 {
                return Err(eval_error_here!("assert.sameValue requires 2 or 3 arguments"));
            }

            let actual = evaluate_expr(env, &args[0])?;
            let expected = evaluate_expr(env, &args[1])?;
            let message = if args.len() == 3 {
                let message_val = evaluate_expr(env, &args[2])?;
                match message_val {
                    Value::String(s) => String::from_utf16_lossy(&s),
                    _ => "assert.sameValue failed".to_string(),
                }
            } else {
                "assert.sameValue failed".to_string()
            };

            // Simple equality check
            let equal = match (&actual, &expected) {
                (Value::Number(a), Value::Number(b)) => a == b,
                (Value::String(a), Value::String(b)) => a == b,
                (Value::Boolean(a), Value::Boolean(b)) => a == b,
                (Value::Undefined, Value::Undefined) => true,
                _ => false, // For simplicity, other types are not equal
            };

            if !equal {
                return Err(eval_error_here!(format!("{message}: expected {expected:?}, got {actual:?}")));
            }

            Ok(Value::Undefined)
        }
        "throws" => {
            // assert.throws(expectedConstructor, func, message?)
            if args.len() < 2 || args.len() > 3 {
                return Err(eval_error_here!("assert.throws requires 2 or 3 arguments"));
            }

            // We only care that calling the provided function throws.
            // Evaluate the second arg (the function) and execute its body.
            let func_val = evaluate_expr(env, &args[1])?;
            match func_val {
                Value::Closure(_params, body, captured_env) => {
                    let func_env = captured_env.clone();
                    match evaluate_statements(&func_env, &body) {
                        Ok(_) => Err(eval_error_here!("assert.throws expected function to throw a value")),
                        Err(_) => Ok(Value::Undefined),
                    }
                }
                _ => Err(eval_error_here!("assert.throws requires a function as the 2nd argument")),
            }
        }
        _ => Err(eval_error_here!(format!("Assert method {method} not implemented"))),
    }
}
