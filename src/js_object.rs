use crate::error::JSError;
use crate::js_array::set_array_length;
use crate::quickjs::{evaluate_expr, obj_set_val, utf8_to_utf16, Expr, JSObjectData, Value};

pub fn handle_object_method(method: &str, args: &[Expr], env: &JSObjectData) -> Result<Value, JSError> {
    match method {
        "keys" => {
            if args.len() == 1 {
                let obj_val = evaluate_expr(env, &args[0])?;
                if let Value::Object(obj) = obj_val {
                    let mut keys = Vec::new();
                    for key in obj.keys() {
                        if key != "length" {
                            // Skip array length property
                            keys.push(Value::String(utf8_to_utf16(key)));
                        }
                    }
                    // Create a simple array-like object for keys
                    let mut result_obj = JSObjectData::new();
                    for (i, key) in keys.into_iter().enumerate() {
                        obj_set_val(&mut result_obj, &i.to_string(), key);
                    }
                    let len = result_obj.len();
                    set_array_length(&mut result_obj, len);
                    Ok(Value::Object(result_obj))
                } else {
                    Err(JSError::EvaluationError {
                        message: "Object.keys expects an object".to_string(),
                    })
                }
            } else {
                Err(JSError::EvaluationError {
                    message: "Object.keys expects exactly one argument".to_string(),
                })
            }
        }
        "values" => {
            if args.len() == 1 {
                let obj_val = evaluate_expr(env, &args[0])?;
                if let Value::Object(obj) = obj_val {
                    let mut values = Vec::new();
                    for (key, value) in obj.iter() {
                        if key != "length" {
                            // Skip array length property
                            values.push(value.clone());
                        }
                    }
                    // Create a simple array-like object for values
                    let mut result_obj = JSObjectData::new();
                    for (i, value) in values.into_iter().enumerate() {
                        obj_set_val(&mut result_obj, &i.to_string(), value.borrow().clone());
                    }
                    let len = result_obj.len();
                    set_array_length(&mut result_obj, len);
                    Ok(Value::Object(result_obj))
                } else {
                    Err(JSError::EvaluationError {
                        message: "Object.values expects an object".to_string(),
                    })
                }
            } else {
                Err(JSError::EvaluationError {
                    message: "Object.values expects exactly one argument".to_string(),
                })
            }
        }
        _ => Err(JSError::EvaluationError {
            message: format!("Object.{} is not implemented", method),
        }),
    }
}
