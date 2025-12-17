use crate::core::{Expr, JSObjectDataPtr, Value, evaluate_expr, evaluate_statements, new_js_object_data, obj_set_key_value};
use crate::error::JSError;

/// Create the assert object with testing functions
pub fn make_assert_object() -> Result<JSObjectDataPtr, JSError> {
    let assert_obj = new_js_object_data();
    obj_set_key_value(&assert_obj, &"sameValue".into(), Value::Function("assert.sameValue".to_string()))?;
    obj_set_key_value(
        &assert_obj,
        &"notSameValue".into(),
        Value::Function("assert.notSameValue".to_string()),
    )?;
    Ok(assert_obj)
}

/// Handle assert object method calls
pub fn handle_assert_method(method: &str, args: &[Expr], env: &JSObjectDataPtr) -> Result<Value, JSError> {
    match method {
        "sameValue" => {
            if args.len() < 2 || args.len() > 3 {
                return Err(raise_eval_error!("assert.sameValue requires 2 or 3 arguments"));
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
                return Err(raise_eval_error!(format!("{message}: expected {expected:?}, got {actual:?}")));
            }

            Ok(Value::Undefined)
        }
        "notSameValue" => {
            if args.len() < 2 || args.len() > 3 {
                return Err(raise_eval_error!("assert.notSameValue requires 2 or 3 arguments"));
            }

            let actual = evaluate_expr(env, &args[0])?;
            let expected = evaluate_expr(env, &args[1])?;
            let message = if args.len() == 3 {
                let message_val = evaluate_expr(env, &args[2])?;
                match message_val {
                    Value::String(s) => String::from_utf16_lossy(&s),
                    _ => "assert.notSameValue failed".to_string(),
                }
            } else {
                "assert.notSameValue failed".to_string()
            };

            // Simple equality check (mirror sameValue logic)
            let equal = match (&actual, &expected) {
                (Value::Number(a), Value::Number(b)) => a == b,
                (Value::String(a), Value::String(b)) => a == b,
                (Value::Boolean(a), Value::Boolean(b)) => a == b,
                (Value::Undefined, Value::Undefined) => true,
                _ => false,
            };

            // If values are the same, this assertion fails — throw a plain error object
            if equal {
                let err_obj = new_js_object_data();
                obj_set_key_value(&err_obj, &"message".into(), Value::String(message.encode_utf16().collect()))?;
                return Err(raise_throw_error!(Value::Object(err_obj)));
            }

            Ok(Value::Undefined)
        }
        "throws" => {
            // assert.throws(expectedConstructor, func, message?)
            if args.len() < 2 || args.len() > 3 {
                return Err(raise_eval_error!("assert.throws requires 2 or 3 arguments"));
            }

            // We only care that calling the provided function throws.
            // Evaluate the second arg (the function) and execute its body.
            let func_val = evaluate_expr(env, &args[1])?;
            match func_val {
                Value::Closure(_params, body, captured_env, _) => {
                    let func_env = new_js_object_data();
                    func_env.borrow_mut().prototype = Some(captured_env.clone());
                    match evaluate_statements(&func_env, &body) {
                        Ok(_) => Err(raise_eval_error!("assert.throws expected function to throw a value")),
                        Err(_) => Ok(Value::Undefined),
                    }
                }
                _ => Err(raise_eval_error!("assert.throws requires a function as the 2nd argument")),
            }
        }
        _ => Err(raise_eval_error!(format!("Assert method {method} not implemented"))),
    }
}
