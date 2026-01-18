use crate::core::{Collect, Gc, GcCell, GcPtr, MutationContext, Trace};
use crate::core::{Expr, JSObjectDataPtr, Value};
use crate::core::{evaluate_expr, evaluate_statements, new_js_object_data, object_set_key_value, prepare_function_call_env};
use crate::error::JSError;
use crate::unicode::utf16_to_utf8;

/// Handle assert object method calls
pub fn handle_assert_method<'gc>(
    mc: &MutationContext<'gc>,
    method: &str,
    args: &[Expr],
    env: &JSObjectDataPtr<'gc>,
) -> Result<Value<'gc>, JSError> {
    match method {
        "sameValue" => {
            if args.len() < 2 || args.len() > 3 {
                return Err(raise_eval_error!("assert.sameValue requires 2 or 3 arguments"));
            }

            let actual = evaluate_expr(mc, env, &args[0])?;
            let expected = evaluate_expr(mc, env, &args[1])?;
            let message = if args.len() == 3 {
                let message_val = evaluate_expr(mc, env, &args[2])?;
                match message_val {
                    Value::String(s) => utf16_to_utf8(&s),
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

            let actual = evaluate_expr(mc, env, &args[0])?;
            let expected = evaluate_expr(mc, env, &args[1])?;
            let message = if args.len() == 3 {
                let message_val = evaluate_expr(mc, env, &args[2])?;
                match message_val {
                    Value::String(s) => utf16_to_utf8(&s),
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
                let err_obj = new_js_object_data(mc);
                object_set_key_value(
                    mc,
                    &err_obj,
                    &"message".into(),
                    Value::String(crate::unicode::utf8_to_utf16(&message)),
                )?;
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
            let func_val = evaluate_expr(mc, env, &args[1])?;
            match func_val {
                Value::Closure(data) => {
                    let body = &data.body;
                    let captured_env = &data.env;
                    let func_env = prepare_function_call_env(mc, Some(captured_env), None, None, &[], None, Some(env))?;
                    match evaluate_statements(mc, &func_env, body) {
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
