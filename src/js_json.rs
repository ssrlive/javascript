use crate::core::MutationContext;
use crate::core::{JSObjectDataPtr, PropertyKey, Value, env_set, get_own_property, new_js_object_data, object_set_key_value};
use crate::error::JSError;
use crate::js_array::{get_array_length, is_array, set_array_length};
use crate::object_get_key_value;
use crate::unicode::{utf8_to_utf16, utf16_to_utf8};

pub fn initialize_json<'gc>(mc: &MutationContext<'gc>, env: &JSObjectDataPtr<'gc>) -> Result<(), JSError> {
    let json_obj = new_js_object_data(mc);

    object_set_key_value(mc, &json_obj, "parse", &Value::Function("JSON.parse".to_string()))?;
    object_set_key_value(mc, &json_obj, "stringify", &Value::Function("JSON.stringify".to_string()))?;

    // JSON object usually has [Symbol.toStringTag] = "JSON"
    // object_set_key_value(mc, &json_obj, "Symbol.toStringTag", &Value::String(utf8_to_utf16("JSON")))?;
    // We can skip that for now if not strictly required, or add it if Symbol is supported.

    env_set(mc, env, "JSON", &Value::Object(json_obj))?;
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
                // Use engine serializer that preserves JS property ordering and skips non-serializable values
                match js_value_to_json_string(mc, &args[0]) {
                    Some(json_str) => Ok(Value::String(utf8_to_utf16(&json_str))),
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
                object_set_key_value(mc, &obj, i, &js_val)?;
            }
            set_array_length(mc, &obj, len)?;
            Ok(Value::Object(obj))
        }
        serde_json::Value::Object(obj) => {
            let js_obj = new_js_object_data(mc);
            for (key, value) in obj.into_iter() {
                let js_val = json_value_to_js_value(mc, value, env)?;
                object_set_key_value(mc, &js_obj, &key, &js_val)?;
            }
            Ok(Value::Object(js_obj))
        }
    }
}

#[allow(dead_code)]
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
                    let val_opt = get_own_property(obj, i);
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
                let ordered = crate::core::ordinary_own_property_keys(obj);
                for key in ordered {
                    if let PropertyKey::String(s) = &key
                        && s != "length"
                        && let Some(val_rc) = object_get_key_value(obj, &key)
                        && let Some(json_val) = js_value_to_json_value(mc, &val_rc.borrow())
                    {
                        map.insert(s.clone(), json_val);
                    }
                }
                Some(serde_json::Value::Object(map))
            }
        }
        _ => None, // Function, Closure not serializable
    }
}

// Minimal JSON stringifier that respects JS property ordering defined by ordinary_own_property_keys.
fn js_value_to_json_string<'gc>(mc: &MutationContext<'gc>, v: &Value<'gc>) -> Option<String> {
    inner(mc, v, 0)
}

fn escape_json_str(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c.is_control() => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out
}

// Inner helper with depth to avoid infinite recursion on cycles
fn inner<'gc>(mc: &MutationContext<'gc>, v: &Value<'gc>, depth: usize) -> Option<String> {
    if depth > 32 {
        return None;
    }
    match v {
        Value::Undefined => None,
        Value::Boolean(b) => Some(if *b { "true".to_string() } else { "false".to_string() }),
        Value::Number(n) => {
            if n.is_finite() {
                if *n == n.trunc() {
                    Some(format!("{}", *n as i64))
                } else {
                    Some(n.to_string())
                }
            } else {
                None
            }
        }
        Value::String(s) => Some(format!("\"{}\"", escape_json_str(&utf16_to_utf8(s)))),
        Value::Object(obj) => {
            if is_array(mc, obj) {
                let len = get_array_length(mc, obj).unwrap_or(obj.borrow().properties.len());
                let mut parts = Vec::with_capacity(len);
                for i in 0..len {
                    if let Some(val_rc) = get_own_property(obj, i) {
                        if let Some(item_str) = inner(mc, &val_rc.borrow(), depth + 1) {
                            parts.push(item_str);
                        } else {
                            parts.push("null".to_string());
                        }
                    } else {
                        parts.push("null".to_string());
                    }
                }
                Some(format!("[{}]", parts.join(",")))
            } else {
                let mut parts: Vec<String> = Vec::new();
                let ordered = crate::core::ordinary_own_property_keys(obj);
                for key in ordered {
                    if let PropertyKey::String(s) = &key {
                        if s == "length" {
                            continue;
                        }
                        if let Some(val_rc) = object_get_key_value(obj, &key) {
                            if let Some(val_str) = inner(mc, &val_rc.borrow(), depth + 1) {
                                parts.push(format!("\"{}\":{}", escape_json_str(s), val_str));
                            } else {
                                // skip undefined/functions or deep/cyclic values
                            }
                        }
                    }
                }
                Some(format!("{{{}}}", parts.join(",")))
            }
        }
        _ => None,
    }
}
