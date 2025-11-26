use crate::error::JSError;
use crate::js_array::{get_array_length, is_array, set_array_length};
use crate::quickjs::{evaluate_expr, obj_get, obj_set_val, utf8_to_utf16, Expr, JSObjectData, Value};

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

pub(crate) fn handle_to_string_method(obj_val: &Value, args: &[Expr]) -> Result<Value, JSError> {
    if !args.is_empty() {
        return Err(JSError::EvaluationError {
            message: format!("{obj_val:?}.toString() takes no arguments, but {} were provided", args.len()),
        });
    }
    match obj_val {
        Value::Number(n) => Ok(Value::String(utf8_to_utf16(&n.to_string()))),
        Value::String(s) => Ok(Value::String(s.clone())),
        Value::Boolean(b) => Ok(Value::String(utf8_to_utf16(&b.to_string()))),
        Value::Undefined => {
            return Err(JSError::EvaluationError {
                message: "TypeError: undefined has no toString method".to_string(),
            });
        }
        Value::Object(ref obj_map) => {
            // If this object looks like an array, join elements with comma
            if is_array(obj_map) {
                let current_len = get_array_length(obj_map).unwrap_or(0);
                let mut parts = Vec::new();
                for i in 0..current_len {
                    if let Some(val_rc) = obj_get(&obj_map, &i.to_string()) {
                        match &*val_rc.borrow() {
                            Value::String(s) => parts.push(String::from_utf16_lossy(s)),
                            Value::Number(n) => parts.push(n.to_string()),
                            Value::Boolean(b) => parts.push(b.to_string()),
                            _ => parts.push("[object Object]".to_string()),
                        }
                    } else {
                        parts.push("".to_string())
                    }
                }
                Ok(Value::String(utf8_to_utf16(&parts.join(","))))
            } else {
                Ok(Value::String(utf8_to_utf16("[object Object]")))
            }
        }
        Value::Function(name) => Ok(Value::String(utf8_to_utf16(&format!("[Function: {}]", name)))),
        Value::Closure(_, _, _) => Ok(Value::String(utf8_to_utf16("[Function]"))),
    }
}
