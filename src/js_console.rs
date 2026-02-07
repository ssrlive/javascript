#![allow(warnings)]

use crate::core::{
    DestructuringElement, EvalError, JSObjectData, JSObjectDataPtr, JSPromise, PromiseState, Value, new_js_object_data,
    object_get_key_value, object_set_key_value,
};
use crate::core::{Gc, GcCell, GcPtr, MutationContext};
use crate::error::JSError;
use crate::js_array::{get_array_length, is_array};
// use crate::js_promise;
use crate::unicode::utf16_to_utf8;
use std::collections::HashSet;

/// Create the console object with logging functions
pub fn initialize_console_object<'gc>(mc: &MutationContext<'gc>) -> Result<JSObjectDataPtr<'gc>, JSError> {
    let console_obj = new_js_object_data(mc);
    object_set_key_value(mc, &console_obj, "log", Value::Function("console.log".to_string()))?;
    // Provide `console.error` as an alias to `console.log` for now
    object_set_key_value(mc, &console_obj, "error", Value::Function("console.error".to_string()))?;
    Ok(console_obj)
}

fn format_console_value<'gc>(
    mc: &MutationContext<'gc>, // added mc
    val: &Value<'gc>,
    env: &JSObjectDataPtr<'gc>,
) -> Result<String, JSError> {
    let mut seen = HashSet::new();
    format_value_pretty(mc, val, env, 0, &mut seen, false)
}

fn format_value_pretty<'gc>(
    mc: &MutationContext<'gc>,
    val: &Value<'gc>,
    _env: &JSObjectDataPtr<'gc>,
    _depth: usize,
    seen: &mut HashSet<*const GcCell<JSObjectData<'gc>>>,
    quote_strings: bool,
) -> Result<String, JSError> {
    match val {
        Value::Number(n) => Ok(n.to_string()),
        Value::BigInt(h) => Ok(format!("{h}n")),
        Value::String(s) => {
            if quote_strings {
                Ok(format!("\"{}\"", crate::unicode::utf16_to_utf8(s)))
            } else {
                Ok(crate::unicode::utf16_to_utf8(s))
            }
        }
        Value::Boolean(b) => Ok(b.to_string()),
        Value::Undefined => Ok("undefined".to_string()),
        Value::Null => Ok("null".to_string()),
        Value::Object(obj) => {
            if let Some(promise) = crate::js_promise::get_promise_from_js_object(obj) {
                return format_promise(mc, &promise, _env, _depth, seen);
            }

            // If object looks like an Error (has non-empty "stack" string), print the stack directly
            if let Some(stack_rc) = object_get_key_value(obj, "stack") {
                if let Value::String(s) = &*stack_rc.borrow() {
                    let s_utf8 = crate::unicode::utf16_to_utf8(s);
                    if !s_utf8.is_empty() {
                        return Ok(s_utf8);
                    }
                }
            }
            if crate::js_regexp::is_regex_object(obj) {
                match crate::js_regexp::get_regex_literal_pattern(obj) {
                    Ok(pat) => Ok(pat),
                    Err(_) => Ok("[object RegExp]".to_string()),
                }
            } else if crate::js_date::is_date_object(obj) {
                // Call toISOString logic manually or directly
                use chrono::{TimeZone, Utc};
                match crate::js_date::internal_get_time_stamp_value(obj) {
                    Some(ts_ptr) => {
                        if let Value::Number(ts) = &*ts_ptr.borrow() {
                            if let Some(dt) = Utc.timestamp_millis_opt(*ts as i64).single() {
                                return Ok(dt.format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string());
                            } else {
                                return Ok("Invalid Date".to_string());
                            }
                        }
                        Ok("Invalid Date".to_string())
                    }
                    None => Ok("[object Date]".to_string()),
                }
            } else if crate::js_array::is_array(mc, obj) {
                if seen.contains(&Gc::as_ptr(*obj)) {
                    return Ok("[Circular]".to_string());
                }
                seen.insert(Gc::as_ptr(*obj));

                let len = crate::js_array::get_array_length(mc, obj).unwrap_or(0);
                let mut s = String::from("[");
                for i in 0..len {
                    if i > 0 {
                        s.push_str(", ");
                    }
                    if let Some(val_rc) = object_get_key_value(obj, i) {
                        let val_str = format_value_pretty(mc, &val_rc.borrow(), _env, _depth + 1, seen, true)?;
                        s.push_str(&val_str);
                    } else if i == len - 1 {
                        s.push(',');
                    }
                }
                s.push(']');
                seen.remove(&Gc::as_ptr(*obj));
                Ok(s)
            } else {
                // If object is a constructor-like function object, print concise "[Function: Name]" like Node
                if let Some(is_ctor_rc) = object_get_key_value(obj, "__is_constructor") {
                    if let Value::Boolean(true) = &*is_ctor_rc.borrow() {
                        // Prefer __native_ctor name
                        if let Some(native_rc) = object_get_key_value(obj, "__native_ctor") {
                            if let Value::String(name_u16) = &*native_rc.borrow() {
                                return Ok(format!("[Function: {}]", utf16_to_utf8(name_u16)));
                            }
                        }
                        // Fallback to 'name' own property
                        if let Some(name_rc) = object_get_key_value(obj, "name") {
                            if let Value::String(name_u16) = &*name_rc.borrow() {
                                let name = utf16_to_utf8(name_u16);
                                if !name.is_empty() {
                                    return Ok(format!("[Function: {}]", name));
                                }
                            }
                        }
                        return Ok("[Function]".to_string());
                    }
                }

                // Check for boxed primitive
                if let Some(val_rc) = object_get_key_value(obj, "__value__") {
                    let val = val_rc.borrow();
                    match *val {
                        Value::Boolean(b) => return Ok(format!("[Boolean: {}]", b)),
                        Value::Number(n) => return Ok(format!("[Number: {}]", n)),
                        Value::String(ref s) => return Ok(format!("[String: '{}']", utf16_to_utf8(s))),
                        Value::BigInt(ref b) => return Ok(format!("[BigInt: {}n]", b)),
                        Value::Symbol(ref s) => return Ok(format!("[Symbol: Symbol({})]", s.description().unwrap_or(""))),
                        _ => {}
                    }
                }

                if seen.contains(&Gc::as_ptr(*obj)) {
                    return Ok("[Circular]".to_string());
                }
                seen.insert(Gc::as_ptr(*obj));

                // Try to get class name
                let mut class_name = String::new();
                if let Some(proto_rc) = obj.borrow().prototype {
                    if let Some(ctor_val_rc) = proto_rc
                        .borrow()
                        .properties
                        .get(&crate::core::PropertyKey::String("constructor".to_string()))
                    {
                        let ctor_val = ctor_val_rc.borrow();
                        if let Value::Object(ctor_obj) = &*ctor_val {
                            if let Some(name_val_rc) = ctor_obj
                                .borrow()
                                .properties
                                .get(&crate::core::PropertyKey::String("name".to_string()))
                            {
                                if let Value::String(name_u16) = &*name_val_rc.borrow() {
                                    let name = utf16_to_utf8(name_u16);
                                    if name != "Object" && !name.is_empty() {
                                        class_name = name;
                                    }
                                }
                            }
                        }
                    }
                }

                let mut s = String::new();
                if !class_name.is_empty() {
                    s.push_str(&class_name);
                    s.push(' ');
                }
                s.push('{');
                let mut first = true;
                for (key, val_rc) in obj.borrow().properties.iter() {
                    let key_str = key.as_ref();
                    if key_str == "__proto__" || key_str == "constructor" || key_str == "__class_def__" {
                        continue;
                    }

                    if !first {
                        s.push_str(", ");
                    }
                    first = false;

                    // Check if key needs quotes
                    let needs_quotes = key_str.is_empty()
                        || key_str.chars().any(|c| c.is_whitespace())
                        || key_str.parse::<f64>().is_ok()
                        || key_str == "[object Object]"
                        || !key_str.chars().all(|c| c.is_alphanumeric() || c == '_' || c == '$');

                    if needs_quotes {
                        s.push_str(&format!("\"{}\"", key_str));
                    } else {
                        s.push_str(key_str);
                    }

                    s.push_str(": ");
                    let val_str = format_value_pretty(mc, &val_rc.borrow(), _env, _depth + 1, seen, true)?;
                    s.push_str(&val_str);
                }
                s.push('}');
                seen.remove(&Gc::as_ptr(*obj));
                Ok(s)
            }
        }
        Value::Function(name) => Ok(format!("function {}() {{ [native code] }}", name)),
        Value::Closure(data) /* | Value::AsyncClosure(data) */ => {
            let params = &data.params;
            let mut s = String::from("function(");
            for (i, param) in params.iter().enumerate() {
                if i > 0 {
                    s.push_str(", ");
                }
                match param {
                    DestructuringElement::Variable(name, _) => s.push_str(name),
                    DestructuringElement::Rest(name) => {
                        s.push_str("...");
                        s.push_str(name);
                    }
                    DestructuringElement::RestPattern(inner) => {
                        s.push_str("...");
                        match &**inner {
                            DestructuringElement::Variable(name, _) => s.push_str(name),
                            DestructuringElement::NestedObject(..) => s.push_str("{}"),
                            DestructuringElement::NestedArray(..) => s.push_str("[]"),
                            _ => {}
                        }
                    }
                    DestructuringElement::NestedObject(..) => s.push_str("{}"),
                    DestructuringElement::NestedArray(..) => s.push_str("[]"),
                    DestructuringElement::Property(name, _) => s.push_str(name),
                    DestructuringElement::ComputedProperty(..) => s.push_str("[]"),
                    DestructuringElement::Empty => {}
                }
            }
            s.push_str(") { [closure code] }");
            Ok(s)
        }
        // Value::ClassDefinition(class_def) => Ok(format!("class {}", class_def.name)),
        // Value::Getter(..) => Ok("[Getter]".to_string()),
        // Value::Setter(..) => Ok("[Setter]".to_string()),
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
        Value::Promise(p_rc) => format_promise(mc, p_rc, _env, _depth, seen),
        Value::Symbol(s) => Ok(format!("Symbol({})", s.description().unwrap_or(""))),
        // Value::Map(_) => Ok("[object Map]".to_string()),
        // Value::Set(_) => Ok("[object Set]".to_string()),
        // Value::WeakMap(_) => Ok("[object WeakMap]".to_string()),
        // Value::WeakSet(_) => Ok("[object WeakSet]".to_string()),
        // Value::GeneratorFunction(..) => Ok("[GeneratorFunction]".to_string()),
        // Value::Generator(_) => Ok("[object Generator]".to_string()),
        Value::Proxy(_) => Ok("[object Proxy]".to_string()),
        // Value::ArrayBuffer(_) => Ok("[object ArrayBuffer]".to_string()),
        // Value::DataView(_) => Ok("[object DataView]".to_string()),
        // Value::TypedArray(_) => Ok("[object TypedArray]".to_string()),
        Value::Uninitialized => Ok("undefined".to_string()),
            _ => Ok("...".to_string()),
            _ => Ok("...".to_string()),
    }
}

// Helper to format a Promise (or an GcPtr<RefCell<JSPromise>>) in Node-like style.
fn format_promise<'gc>(
    mc: &MutationContext<'gc>,
    p_rc: &GcPtr<'gc, JSPromise<'gc>>,
    _env: &JSObjectDataPtr<'gc>,
    depth: usize,
    seen: &mut HashSet<*const GcCell<JSObjectData<'gc>>>,
) -> Result<String, JSError> {
    let p = p_rc.borrow();
    match &p.state {
        PromiseState::Pending => Ok("Promise { <pending> }".to_string()),
        PromiseState::Fulfilled(val) => {
            let inner = format_value_pretty(mc, val, _env, depth + 1, seen, false)?;
            Ok(format!("Promise {{ {} }}", inner))
        }
        PromiseState::Rejected(val) => {
            let inner = format_value_pretty(mc, val, _env, depth + 1, seen, false)?;
            Ok(format!("Promise {{ <rejected> {} }}", inner))
        }
    }
}

/// Handle console object method calls
pub fn handle_console_method<'gc>(
    mc: &MutationContext<'gc>, // added mc
    method: &str,
    values: &[Value<'gc>],
    env: &JSObjectDataPtr<'gc>,
) -> Result<Value<'gc>, crate::core::EvalError<'gc>> {
    match method {
        "log" | "error" => {
            // Instrument: record current tick and task-queue length when console.log is invoked
            // log::debug!(
            //     "console.log called; CURRENT_TICK={} task_queue_len={}",
            //     js_promise::current_tick(),
            //     js_promise::task_queue_len()
            // );

            if values.is_empty() {
                println!();
                return Ok(Value::Undefined);
            }

            let mut output = String::new();
            let mut arg_idx = 0;

            // Check for format string
            let mut formatted = false;
            if let Value::String(s_utf16) = &values[0] {
                let s = utf16_to_utf8(s_utf16);
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
                                                's' => output.push_str(&format_console_value(mc, val, env)?),
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
                                                    output.push_str(&format_console_value(mc, val, env)?);
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
                output.push_str(&format_console_value(mc, &values[0], env)?);
                arg_idx = 0;
            }

            // Print remaining args
            for values_i in &values[(arg_idx + 1)..] {
                if !output.is_empty() {
                    output.push(' ');
                }
                output.push_str(&format_console_value(mc, values_i, env)?);
            }

            println!("{}", output);
            Ok(Value::Undefined)
        }
        _ => Err(raise_eval_error!(format!("Console method {method} not implemented")).into()),
    }
}

/// Print additional own non-index properties of an array object
/// Not enabled by default; can be called from handle_console_method if desired
fn _print_additional_info_for_array<'gc>(mc: &MutationContext<'gc>, obj: &JSObjectDataPtr<'gc>) -> Result<(), crate::core::EvalError<'gc>> {
    // Collect and print own non-index properties.
    // Print common RegExp-related props in a stable order for readability.

    let Some(len) = get_array_length(mc, obj) else {
        return Ok(());
    };

    let mut printed_any = false;
    let mut need_sep = len > 0;

    // Helper to print a single property if present
    let mut print_prop = |k: &str| -> Result<bool, crate::core::EvalError<'gc>> {
        if let Some(vrc) = object_get_key_value(obj, k) {
            if need_sep {
                print!(", ");
            }
            need_sep = true;
            printed_any = true;
            print!("{}: ", k);
            match &*vrc.borrow() {
                Value::Number(n) => print!("{}", n),
                Value::BigInt(h) => print!("{h}"),
                Value::String(s) => print!("'{}'", utf16_to_utf8(s)),
                Value::Boolean(b) => print!("{}", b),
                Value::Undefined => print!("undefined"),
                Value::Null => print!("null"),
                Value::Object(inner_obj) => {
                    if is_array(mc, inner_obj) {
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
            Value::String(s) => print!("'{}'", utf16_to_utf8(s)),
            Value::Boolean(b) => print!("{}", b),
            Value::Undefined => print!("undefined"),
            Value::Null => print!("null"),
            Value::Object(inner_obj) => {
                if is_array(mc, inner_obj) {
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
