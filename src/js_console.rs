use crate::core::{Expr, JSObjectDataPtr, Value, evaluate_expr, new_js_object_data, obj_get_key_value, obj_set_key_value};
use crate::error::JSError;
use crate::js_promise;

/// Create the console object with logging functions
pub fn make_console_object() -> Result<JSObjectDataPtr, JSError> {
    let console_obj = new_js_object_data();
    obj_set_key_value(&console_obj, &"log".into(), Value::Function("console.log".to_string()))?;
    Ok(console_obj)
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
            // console.log call
            let count = args.len();
            for (i, arg) in args.iter().enumerate() {
                let arg_val = evaluate_expr(env, arg)?;
                match arg_val {
                    Value::Number(n) => print!("{}", n),
                    Value::BigInt(h) => print!("{h}"),
                    Value::String(s) => {
                        print!("{}", String::from_utf16_lossy(&s))
                    }
                    Value::Boolean(b) => print!("{}", b),
                    Value::Undefined => print!("undefined"),
                    Value::Null => print!("null"),
                    Value::Object(obj) => {
                        // Check if this is a RegExp object
                        if crate::js_regexp::is_regex_object(&obj) {
                            // Print regex in /pattern/flags form
                            match crate::js_regexp::get_regex_literal_pattern(&obj) {
                                Ok(pat) => print!("{}", pat),
                                Err(_) => print!("[object RegExp]"),
                            }
                        } else if crate::js_date::is_date_object(&obj) {
                            // For Date objects, call toString method
                            match crate::js_date::handle_date_method(&obj, "toISOString", &[], env) {
                                Ok(Value::String(s)) => print!("{}", String::from_utf16_lossy(&s)),
                                _ => print!("[object Date]"),
                            }
                        } else if crate::js_array::is_array(&obj) {
                            // Print array contents
                            let len = crate::js_array::get_array_length(&obj).unwrap_or(0);
                            print!("[");
                            // Print elements
                            for i in 0..len {
                                if i > 0 {
                                    print!(", ");
                                }
                                if let Some(val_rc) = obj_get_key_value(&obj, &i.to_string().into())? {
                                    match &*val_rc.borrow() {
                                        Value::Number(n) => print!("{}", n),
                                        Value::BigInt(h) => print!("{h}"),
                                        Value::String(s) => print!("'{}'", String::from_utf16_lossy(s)),
                                        Value::Boolean(b) => print!("{}", b),
                                        Value::Undefined => print!("undefined"),
                                        Value::Null => print!("null"),
                                        _ => print!("[object Object]"),
                                    }
                                } else {
                                    // missing element -> print nothing (sparse arrays not shown)
                                }
                            }

                            // Print additional own non-index properties, not enabled by default
                            // _print_additional_info_for_array(&obj)?;

                            print!("]");
                        } else {
                            // Print object properties
                            print!("{{");
                            let mut first = true;
                            for (key, val_rc) in obj.borrow().properties.iter() {
                                if !first {
                                    print!(", ");
                                }
                                first = false;
                                print!("{}: ", key);
                                match &*val_rc.borrow() {
                                    Value::Number(n) => print!("{}", n),
                                    Value::String(s) => print!("\"{}\"", String::from_utf16_lossy(s)),
                                    Value::Boolean(b) => print!("{}", b),
                                    Value::Undefined => print!("undefined"),
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
                            print!("}}");
                        }
                    }
                    Value::Function(name) => print!("function {}() {{ [native code] }}", name),
                    Value::Closure(params, ..) | Value::AsyncClosure(params, _, _, _) => {
                        print!("function(");
                        for (i, param) in params.iter().enumerate() {
                            if i > 0 {
                                print!(", ");
                            }
                            print!("{}", param.0);
                        }
                        print!(") {{ [closure code] }}");
                    }
                    Value::ClassDefinition(ref class_def) => print!("class {}", class_def.name),
                    Value::Getter(..) => print!("[Getter]"),
                    Value::Setter(..) => print!("[Setter]"),
                    Value::Property { value, getter, setter } => {
                        print!("[Property");
                        if value.is_some() {
                            print!(" value");
                        }
                        if getter.is_some() {
                            print!(" getter");
                        }
                        if setter.is_some() {
                            print!(" setter");
                        }
                        print!("]");
                    }
                    Value::Promise(_) => print!("[object Promise]"),
                    Value::Symbol(_) => print!("[object Symbol]"),
                    Value::Map(_) => print!("[object Map]"),
                    Value::Set(_) => print!("[object Set]"),
                    Value::WeakMap(_) => print!("[object WeakMap]"),
                    Value::WeakSet(_) => print!("[object WeakSet]"),
                    Value::GeneratorFunction(..) => print!("[GeneratorFunction]"),
                    Value::Generator(_) => print!("[object Generator]"),
                    Value::Proxy(_) => print!("[object Proxy]"),
                    Value::ArrayBuffer(_) => print!("[object ArrayBuffer]"),
                    Value::DataView(_) => print!("[object DataView]"),
                    Value::TypedArray(_) => print!("[object TypedArray]"),
                }
                if i < count - 1 {
                    print!(" ");
                }
            }
            println!();
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
