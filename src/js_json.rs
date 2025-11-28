use crate::core::{evaluate_expr, obj_set_value, utf16_to_utf8, utf8_to_utf16, Expr, JSObjectData, JSObjectDataPtr, Value};
use crate::error::JSError;
use crate::js_array::{is_array, set_array_length};
use std::cell::RefCell;
use std::rc::Rc;

pub fn handle_json_method(method: &str, args: &[Expr], env: &JSObjectDataPtr) -> Result<Value, JSError> {
    match method {
        "parse" => {
            if args.len() == 1 {
                let arg_val = evaluate_expr(env, &args[0])?;
                match arg_val {
                    Value::String(s) => {
                        let json_str = utf16_to_utf8(&s);
                        match serde_json::from_str::<serde_json::Value>(&json_str) {
                            Ok(json_value) => json_value_to_js_value(json_value),
                            Err(_) => Err(JSError::EvaluationError {
                                message: "Invalid JSON".to_string(),
                            }),
                        }
                    }
                    _ => Err(JSError::EvaluationError {
                        message: "JSON.parse expects a string".to_string(),
                    }),
                }
            } else {
                Err(JSError::EvaluationError {
                    message: "JSON.parse expects exactly one argument".to_string(),
                })
            }
        }
        "stringify" => {
            if args.len() == 1 {
                let arg_val = evaluate_expr(env, &args[0])?;
                match js_value_to_json_value(arg_val) {
                    Some(json_value) => match serde_json::to_string(&json_value) {
                        Ok(json_str) => Ok(Value::String(utf8_to_utf16(&json_str))),
                        Err(_) => Ok(Value::Undefined),
                    },
                    None => Ok(Value::Undefined),
                }
            } else {
                Err(JSError::EvaluationError {
                    message: "JSON.stringify expects exactly one argument".to_string(),
                })
            }
        }
        _ => Err(JSError::EvaluationError {
            message: format!("JSON.{} is not implemented", method),
        }),
    }
}

fn json_value_to_js_value(json_value: serde_json::Value) -> Result<Value, JSError> {
    match json_value {
        serde_json::Value::Null => Ok(Value::Undefined),
        serde_json::Value::Bool(b) => Ok(Value::Boolean(b)),
        serde_json::Value::Number(n) => {
            if let Some(f) = n.as_f64() {
                Ok(Value::Number(f))
            } else {
                Ok(Value::Undefined)
            }
        }
        serde_json::Value::String(s) => Ok(Value::String(utf8_to_utf16(&s))),
        serde_json::Value::Array(arr) => {
            let len = arr.len();
            let obj = Rc::new(RefCell::new(JSObjectData::new()));
            for (i, item) in arr.into_iter().enumerate() {
                let js_val = json_value_to_js_value(item)?;
                obj_set_value(&obj, &i.to_string(), js_val)?;
            }
            set_array_length(&obj, len)?;
            Ok(Value::Object(obj))
        }
        serde_json::Value::Object(obj) => {
            let js_obj = Rc::new(RefCell::new(JSObjectData::new()));
            for (key, value) in obj.into_iter() {
                let js_val = json_value_to_js_value(value)?;
                obj_set_value(&js_obj, &key, js_val)?;
            }
            Ok(Value::Object(js_obj))
        }
    }
}

fn js_value_to_json_value(js_value: Value) -> Option<serde_json::Value> {
    match js_value {
        Value::Undefined => Some(serde_json::Value::Null),
        Value::Boolean(b) => Some(serde_json::Value::Bool(b)),
        Value::Number(n) => {
            if n.is_finite() {
                if n == n.trunc() {
                    // Integer
                    Some(serde_json::Value::Number(serde_json::Number::from(n as i64)))
                } else {
                    Some(serde_json::Value::Number(serde_json::Number::from_f64(n)?))
                }
            } else {
                None
            }
        }
        Value::String(s) => {
            let utf8_str = utf16_to_utf8(&s);
            Some(serde_json::Value::String(utf8_str))
        }
        Value::Object(obj) => {
            if is_array(&obj) {
                let len = obj.borrow().properties.len();
                let mut arr = Vec::new();
                for i in 0..len {
                    if let Some(val) = obj.borrow().get(&i.to_string()) {
                        if let Some(json_val) = js_value_to_json_value(val.borrow().clone()) {
                            arr.push(json_val);
                        } else {
                            return None;
                        }
                    } else {
                        return None;
                    }
                }
                Some(serde_json::Value::Array(arr))
            } else {
                let mut map = serde_json::Map::new();
                for (key, value) in obj.borrow().properties.iter() {
                    if key != "length" {
                        if let Some(json_val) = js_value_to_json_value(value.borrow().clone()) {
                            map.insert(key.clone(), json_val);
                        } else {
                            return None;
                        }
                    }
                }
                Some(serde_json::Value::Object(map))
            }
        }
        _ => None, // Function, Closure not serializable
    }
}
