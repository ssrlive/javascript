use crate::core::{Expr, JSObjectDataPtr, Value, evaluate_expr, new_js_object_data, obj_get_key_value, obj_set_key_value};
use crate::error::JSError;
use crate::js_promise;

/// Create the console object with logging functions
pub fn make_console_object() -> Result<JSObjectDataPtr, JSError> {
    let console_obj = new_js_object_data();
    obj_set_key_value(&console_obj, &"log".into(), Value::Function("console.log".to_string()))?;
    Ok(console_obj)
}

fn format_console_value(val: &Value, env: &JSObjectDataPtr) -> Result<String, JSError> {
    match val {
        Value::Number(n) => Ok(n.to_string()),
        Value::BigInt(h) => Ok(h.to_string()),
        Value::String(s) => Ok(String::from_utf16_lossy(s)),
        Value::Boolean(b) => Ok(b.to_string()),
        Value::Undefined => Ok("undefined".to_string()),
        Value::Null => Ok("null".to_string()),
        Value::Object(obj) => {
            if crate::js_regexp::is_regex_object(obj) {
                match crate::js_regexp::get_regex_literal_pattern(obj) {
                    Ok(pat) => Ok(pat),
                    Err(_) => Ok("[object RegExp]".to_string()),
                }
            } else if crate::js_date::is_date_object(obj) {
                match crate::js_date::handle_date_method(obj, "toISOString", &[], env) {
                    Ok(Value::String(s)) => Ok(String::from_utf16_lossy(&s)),
                    _ => Ok("[object Date]".to_string()),
                }
            } else if crate::js_array::is_array(obj) {
                let len = crate::js_array::get_array_length(obj).unwrap_or(0);
                let mut s = String::from("[");
                for i in 0..len {
                    if i > 0 {
                        s.push_str(", ");
                    }
                    if let Some(val_rc) = obj_get_key_value(obj, &i.to_string().into())? {
                        match &*val_rc.borrow() {
                            Value::Number(n) => s.push_str(&n.to_string()),
                            Value::BigInt(h) => s.push_str(&h.to_string()),
                            Value::String(str_val) => {
                                s.push('\'');
                                s.push_str(&String::from_utf16_lossy(str_val));
                                s.push('\'');
                            }
                            Value::Boolean(b) => s.push_str(&b.to_string()),
                            Value::Undefined => s.push_str("undefined"),
                            Value::Null => s.push_str("null"),
                            _ => s.push_str("[object Object]"),
                        }
                    }
                }
                s.push(']');
                Ok(s)
            } else {
                let mut s = String::from("{");
                let mut first = true;
                for (key, val_rc) in obj.borrow().properties.iter() {
                    if !first {
                        s.push_str(", ");
                    }
                    first = false;
                    s.push_str(key.as_ref());
                    s.push_str(": ");
                    match &*val_rc.borrow() {
                        Value::Number(n) => s.push_str(&n.to_string()),
                        Value::String(str_val) => {
                            s.push('"');
                            s.push_str(&String::from_utf16_lossy(str_val));
                            s.push('"');
                        }
                        Value::Boolean(b) => s.push_str(&b.to_string()),
                        Value::Undefined => s.push_str("undefined"),
                        Value::Object(inner_obj) => {
                            if crate::js_array::is_array(inner_obj) {
                                s.push_str("[Array]");
                            } else {
                                s.push_str("[object Object]");
                            }
                        }
                        _ => s.push_str("[object Object]"),
                    }
                }
                s.push('}');
                Ok(s)
            }
        }
        Value::Function(name) => Ok(format!("function {}() {{ [native code] }}", name)),
        Value::Closure(params, ..) | Value::AsyncClosure(params, _, _, _) => {
            let mut s = String::from("function(");
            for (i, param) in params.iter().enumerate() {
                if i > 0 {
                    s.push_str(", ");
                }
                s.push_str(&param.0);
            }
            s.push_str(") { [closure code] }");
            Ok(s)
        }
        Value::ClassDefinition(class_def) => Ok(format!("class {}", class_def.name)),
        Value::Getter(..) => Ok("[Getter]".to_string()),
        Value::Setter(..) => Ok("[Setter]".to_string()),
        Value::Property { value, getter, setter } => {
            let mut s = String::from("[Property");
            if value.is_some() {
                s.push_str(" value");
            }
            if getter.is_some() {
                s.push_str(" getter");
            }
            if setter.is_some() {
                s.push_str(" setter");
            }
            s.push(']');
            Ok(s)
        }
        Value::Promise(_) => Ok("[object Promise]".to_string()),
        Value::Symbol(_) => Ok("[object Symbol]".to_string()),
        Value::Map(_) => Ok("[object Map]".to_string()),
        Value::Set(_) => Ok("[object Set]".to_string()),
        Value::WeakMap(_) => Ok("[object WeakMap]".to_string()),
        Value::WeakSet(_) => Ok("[object WeakSet]".to_string()),
        Value::GeneratorFunction(..) => Ok("[GeneratorFunction]".to_string()),
        Value::Generator(_) => Ok("[object Generator]".to_string()),
        Value::Proxy(_) => Ok("[object Proxy]".to_string()),
        Value::ArrayBuffer(_) => Ok("[object ArrayBuffer]".to_string()),
        Value::DataView(_) => Ok("[object DataView]".to_string()),
        Value::TypedArray(_) => Ok("[object TypedArray]".to_string()),
    }
}

/// Handle console object method calls
pub fn handle_console_method(method: &str, args: &[Expr], env: &JSObjectDataPtr) -> Result<Value, JSError> {
    match method {
        "log" => {
            // Instrument: record current tick and task-queue length when console.log is invoked
            log::debug!(
                "console.log called; CURRENT_TICK={} task_queue_len={}",
                js_promise::current_tick(),
                js_promise::task_queue_len()
            );

            let mut values = Vec::new();
            for arg in args {
                values.push(evaluate_expr(env, arg)?);
            }

            if values.is_empty() {
                println!();
                return Ok(Value::Undefined);
            }

            let mut output = String::new();
            let mut arg_idx = 0;

            // Check for format string
            let mut formatted = false;
            if let Value::String(s_utf16) = &values[0] {
                let s = String::from_utf16_lossy(s_utf16);
                if s.contains('%') && values.len() > 1 {
                    formatted = true;
                    let mut chars = s.chars().peekable();
                    while let Some(c) = chars.next() {
                        if c == '%' {
                            if let Some(&next_char) = chars.peek() {
                                match next_char {
                                    's' | 'd' | 'i' | 'f' | 'o' | 'O' | 'c' => {
                                        chars.next(); // consume specifier
                                        arg_idx += 1;
                                        if arg_idx < values.len() {
                                            let val = &values[arg_idx];
                                            match next_char {
                                                's' => output.push_str(&format_console_value(val, env)?),
                                                'd' | 'i' => {
                                                    if let Value::Number(n) = val {
                                                        output.push_str(&format!("{:.0}", n));
                                                    } else {
                                                        output.push_str("NaN");
                                                    }
                                                }
                                                'f' => {
                                                    if let Value::Number(n) = val {
                                                        output.push_str(&n.to_string());
                                                    } else {
                                                        output.push_str("NaN");
                                                    }
                                                }
                                                'o' | 'O' => {
                                                    output.push_str(&format_console_value(val, env)?);
                                                }
                                                'c' => {
                                                    // Ignore CSS
                                                }
                                                _ => {}
                                            }
                                        } else {
                                            output.push('%');
                                            output.push(next_char);
                                        }
                                    }
                                    '%' => {
                                        chars.next();
                                        output.push('%');
                                    }
                                    _ => {
                                        output.push('%');
                                    }
                                }
                            } else {
                                output.push('%');
                            }
                        } else {
                            output.push(c);
                        }
                    }
                }
            }

            if !formatted {
                // Just print first arg
                output.push_str(&format_console_value(&values[0], env)?);
                arg_idx = 0;
            }

            // Print remaining args
            for values_i in &values[(arg_idx + 1)..] {
                if !output.is_empty() {
                    output.push(' ');
                }
                output.push_str(&format_console_value(values_i, env)?);
            }

            println!("{}", output);
            Ok(Value::Undefined)
        }
        _ => Err(raise_eval_error!(format!("Console method {method} not implemented"))),
    }
}

/// Print additional own non-index properties of an array object
/// Not enabled by default; can be called from handle_console_method if desired
fn _print_additional_info_for_array(obj: &JSObjectDataPtr) -> Result<(), JSError> {
    // Collect and print own non-index properties.
    // Print common RegExp-related props in a stable order for readability.

    let Some(len) = crate::js_array::get_array_length(obj) else {
        return Ok(());
    };

    let mut printed_any = false;
    let mut need_sep = len > 0;

    // Helper to print a single property if present
    let mut print_prop = |k: &str| -> Result<bool, JSError> {
        if let Some(vrc) = obj_get_key_value(obj, &k.into())? {
            if need_sep {
                print!(", ");
            }
            need_sep = true;
            printed_any = true;
            print!("{}: ", k);
            match &*vrc.borrow() {
                Value::Number(n) => print!("{}", n),
                Value::BigInt(h) => print!("{h}"),
                Value::String(s) => print!("'{}'", String::from_utf16_lossy(s)),
                Value::Boolean(b) => print!("{}", b),
                Value::Undefined => print!("undefined"),
                Value::Null => print!("null"),
                Value::Object(inner_obj) => {
                    if crate::js_array::is_array(inner_obj) {
                        print!("[Array]");
                    } else {
                        print!("[object Object]");
                    }
                }
                _ => print!("[object Object]"),
            }
            return Ok(true);
        }
        Ok(false)
    };

    // stable order: index, input, groups
    let _ = print_prop("index")?;
    let _ = print_prop("input")?;
    let _ = print_prop("groups")?;

    // Now print any other own string-key properties not already printed and not numeric indices
    for (key, val_rc) in obj.borrow().properties.iter() {
        // skip length and already-printed common props and numeric indices
        let skip = match key {
            crate::core::PropertyKey::String(s) if s == "length" => true,
            crate::core::PropertyKey::String(s) => {
                if s == "index" || s == "input" || s == "groups" {
                    true
                } else if let Ok(idx) = s.parse::<usize>() {
                    idx < len
                } else {
                    false
                }
            }
            _ => false,
        };
        if skip {
            continue;
        }

        // print separator
        if need_sep {
            print!(", ");
        }
        need_sep = true;

        // key: value -- rely on Display for key
        print!("{}: ", key);
        match &*val_rc.borrow() {
            Value::Number(n) => print!("{}", n),
            Value::BigInt(h) => print!("{h}"),
            Value::String(s) => print!("'{}'", String::from_utf16_lossy(s)),
            Value::Boolean(b) => print!("{}", b),
            Value::Undefined => print!("undefined"),
            Value::Null => print!("null"),
            Value::Object(inner_obj) => {
                if crate::js_array::is_array(inner_obj) {
                    print!("[Array]");
                } else {
                    print!("[object Object]");
                }
            }
            _ => print!("[object Object]"),
        }
    }
    Ok(())
}
