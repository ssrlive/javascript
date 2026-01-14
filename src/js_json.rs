use crate::core::MutationContext;
use crate::core::{JSObjectDataPtr, PropertyKey, Value, env_set, get_own_property, new_js_object_data, obj_set_key_value};
use crate::error::JSError;
use crate::js_array::{get_array_length, is_array, set_array_length};
use crate::unicode::{utf8_to_utf16, utf16_to_utf8};

pub fn initialize_json<'gc>(mc: &MutationContext<'gc>, env: &JSObjectDataPtr<'gc>) -> Result<(), JSError> {
    let json_obj = new_js_object_data(mc);

    obj_set_key_value(mc, &json_obj, &"parse".into(), Value::Function("JSON.parse".to_string()))?;
    obj_set_key_value(mc, &json_obj, &"stringify".into(), Value::Function("JSON.stringify".to_string()))?;

    // JSON object usually has [Symbol.toStringTag] = "JSON"
    // obj_set_key_value(mc, &json_obj, &crate::core::PropertyKey::from("Symbol.toStringTag"), Value::String(utf8_to_utf16("JSON")))?;
    // We can skip that for now if not strictly required, or add it if Symbol is supported.

    env_set(mc, env, "JSON", Value::Object(json_obj))?;
    Ok(())
}

pub fn handle_json_method<'gc>(
    mc: &MutationContext<'gc>,
    method: &str,
    args: &[Value<'gc>],
    env: &JSObjectDataPtr<'gc>,
) -> Result<Value<'gc>, JSError> {
    match method {
        "parse" => {
            if !args.is_empty() {
                let arg_val = &args[0];
                match arg_val {
                    Value::String(s) => {
                        let json_str = utf16_to_utf8(s);
                        match serde_json::from_str::<serde_json::Value>(&json_str) {
                            Ok(json_value) => json_value_to_js_value(mc, json_value, env),
                            Err(_) => Err(raise_eval_error!("Invalid JSON")),
                        }
                    }
                    _ => Err(raise_eval_error!("JSON.parse expects a string")),
                }
            } else {
                Err(raise_eval_error!("JSON.parse expects exactly one argument"))
            }
        }
        "stringify" => {
            if !args.is_empty() {
                match js_value_to_json_value(mc, &args[0]) {
                    Some(json_value) => match serde_json::to_string(&json_value) {
                        Ok(json_str) => {
                            log::debug!("JSON.stringify produced: {}", json_str);
                            Ok(Value::String(utf8_to_utf16(&json_str)))
                        }
                        Err(_) => Ok(Value::Undefined),
                    },
                    None => Ok(Value::Undefined),
                }
            } else {
                Err(raise_eval_error!("JSON.stringify expects exactly one argument"))
            }
        }
        _ => Err(raise_eval_error!(format!("JSON.{method} is not implemented"))),
    }
}

fn json_value_to_js_value<'gc>(
    mc: &MutationContext<'gc>,
    json_value: serde_json::Value,
    env: &JSObjectDataPtr<'gc>,
) -> Result<Value<'gc>, JSError> {
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
            let obj = crate::js_array::create_array(mc, env)?;
            for (i, item) in arr.into_iter().enumerate() {
                let js_val = json_value_to_js_value(mc, item, env)?;
                obj_set_key_value(mc, &obj, &i.to_string().into(), js_val)?;
            }
            set_array_length(mc, &obj, len)?;
            Ok(Value::Object(obj))
        }
        serde_json::Value::Object(obj) => {
            let js_obj = new_js_object_data(mc);
            for (key, value) in obj.into_iter() {
                let js_val = json_value_to_js_value(mc, value, env)?;
                obj_set_key_value(mc, &js_obj, &key.into(), js_val)?;
            }
            Ok(Value::Object(js_obj))
        }
    }
}

fn js_value_to_json_value<'gc>(mc: &MutationContext<'gc>, js_value: &Value<'gc>) -> Option<serde_json::Value> {
    match js_value {
        Value::Undefined => None,
        Value::Boolean(b) => Some(serde_json::Value::Bool(*b)),
        Value::Number(n) => {
            if n.is_finite() {
                if *n == n.trunc() {
                    // Integer
                    Some(serde_json::Value::Number(serde_json::Number::from(*n as i64)))
                } else {
                    Some(serde_json::Value::Number(serde_json::Number::from_f64(*n)?))
                }
            } else {
                None
            }
        }
        Value::String(s) => {
            let utf8_str = utf16_to_utf8(s);
            Some(serde_json::Value::String(utf8_str))
        }
        Value::Object(obj) => {
            if is_array(mc, obj) {
                let len = get_array_length(mc, obj).unwrap_or(obj.borrow().properties.len());
                log::debug!("js_value_to_json_value: array with properties.len() = {}", len);
                let mut arr = Vec::new();
                for i in 0..len {
                    let val_opt = get_own_property(obj, &i.to_string().into());
                    if let Some(val_rc) = &val_opt {
                        if let Some(json_val) = js_value_to_json_value(mc, &val_rc.borrow()) {
                            arr.push(json_val);
                        } else {
                            // Undefined, Function, Symbol in array -> null
                            arr.push(serde_json::Value::Null);
                        }
                    } else {
                        // Hole -> null
                        arr.push(serde_json::Value::Null);
                    }
                }
                Some(serde_json::Value::Array(arr))
            } else {
                let mut map = serde_json::Map::new();
                for (key, value) in obj.borrow().properties.iter() {
                    if let PropertyKey::String(s) = key
                        && s != "length"
                    {
                        if let Some(json_val) = js_value_to_json_value(mc, &value.borrow()) {
                            map.insert(s.clone(), json_val);
                        } else {
                            // If None (undefined, function, etc), skip property
                        }
                    }
                }
                Some(serde_json::Value::Object(map))
            }
        }
        _ => None, // Function, Closure not serializable
    }
}
