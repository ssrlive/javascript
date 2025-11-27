use crate::error::JSError;
use crate::js_array::{get_array_length, is_array, set_array_length};
use crate::quickjs::{evaluate_expr, obj_get, obj_set_val, utf8_to_utf16, Expr, JSObjectData, JSObjectDataPtr, Value};
use std::cell::RefCell;
use std::rc::Rc;

pub fn handle_object_method(method: &str, args: &[Expr], env: &JSObjectDataPtr) -> Result<Value, JSError> {
    match method {
        "keys" => {
            if args.is_empty() {
                return Err(JSError::TypeError {
                    message: "Object.keys requires at least one argument".to_string(),
                });
            }
            if args.len() > 1 {
                return Err(JSError::TypeError {
                    message: "Object.keys accepts only one argument".to_string(),
                });
            }
            let obj_val = evaluate_expr(env, &args[0])?;
            match obj_val {
                Value::Object(obj) => {
                    let mut keys = Vec::new();
                    for key in obj.borrow().keys() {
                        if key != "length" {
                            // Skip array length property
                            keys.push(Value::String(utf8_to_utf16(key)));
                        }
                    }
                    // Create a simple array-like object for keys
                    let result_obj = Rc::new(RefCell::new(JSObjectData::new()));
                    for (i, key) in keys.into_iter().enumerate() {
                        obj_set_val(&result_obj, &i.to_string(), key);
                    }
                    let len = result_obj.borrow().properties.len();
                    set_array_length(&result_obj, len);
                    Ok(Value::Object(result_obj))
                }
                Value::Undefined => {
                    return Err(JSError::TypeError {
                        message: "Object.keys called on undefined".to_string(),
                    });
                }
                _ => {
                    // For primitive values, return empty array (like in JS)
                    let result_obj = Rc::new(RefCell::new(JSObjectData::new()));
                    set_array_length(&result_obj, 0);
                    Ok(Value::Object(result_obj))
                }
            }
        }
        "values" => {
            if args.is_empty() {
                return Err(JSError::TypeError {
                    message: "Object.values requires at least one argument".to_string(),
                });
            }
            if args.len() > 1 {
                return Err(JSError::TypeError {
                    message: "Object.values accepts only one argument".to_string(),
                });
            }
            let obj_val = evaluate_expr(env, &args[0])?;
            match obj_val {
                Value::Object(obj) => {
                    let mut values = Vec::new();
                    for (key, value) in obj.borrow().properties.iter() {
                        if key != "length" {
                            // Skip array length property
                            values.push(value.borrow().clone());
                        }
                    }
                    // Create a simple array-like object for values
                    let result_obj = Rc::new(RefCell::new(JSObjectData::new()));
                    for (i, value) in values.into_iter().enumerate() {
                        obj_set_val(&result_obj, &i.to_string(), value);
                    }
                    let len = result_obj.borrow().properties.len();
                    set_array_length(&result_obj, len);
                    Ok(Value::Object(result_obj))
                }
                Value::Undefined => {
                    return Err(JSError::TypeError {
                        message: "Object.values called on undefined".to_string(),
                    });
                }
                _ => {
                    // For primitive values, return empty array (like in JS)
                    let result_obj = Rc::new(RefCell::new(JSObjectData::new()));
                    set_array_length(&result_obj, 0);
                    Ok(Value::Object(result_obj))
                }
            }
        }
        _ => Err(JSError::EvaluationError {
            message: format!("Object.{} is not implemented", method),
        }),
    }
}

pub(crate) fn handle_to_string_method(obj_val: &Value, args: &[Expr]) -> Result<Value, JSError> {
    if !args.is_empty() {
        return Err(JSError::TypeError {
            message: format!(
                "{}.toString() takes no arguments, but {} were provided",
                match obj_val {
                    Value::Number(_) => "Number",
                    Value::String(_) => "String",
                    Value::Boolean(_) => "Boolean",
                    Value::Object(_) => "Object",
                    Value::Function(_) => "Function",
                    Value::Closure(_, _, _) => "Function",
                    Value::Undefined => "undefined",
                    Value::ClassDefinition(_) => "Class",
                },
                args.len()
            ),
        });
    }
    match obj_val {
        Value::Number(n) => Ok(Value::String(utf8_to_utf16(&n.to_string()))),
        Value::String(s) => Ok(Value::String(s.clone())),
        Value::Boolean(b) => Ok(Value::String(utf8_to_utf16(&b.to_string()))),
        Value::Undefined => {
            return Err(JSError::TypeError {
                message: "Cannot convert undefined to object".to_string(),
            });
        }
        Value::Object(ref obj_map) => {
            // If this object looks like a Date (has __timestamp), call Date.toString()
            if obj_map.borrow().contains_key("__timestamp") {
                return crate::js_date::handle_date_method(&*obj_map.borrow(), "toString", args);
            }
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
        Value::ClassDefinition(_) => Ok(Value::String(utf8_to_utf16("[Class]"))),
    }
}
