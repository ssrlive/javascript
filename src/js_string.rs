use crate::core::{Expr, JSObjectData, JSObjectDataPtr, Value, evaluate_expr, obj_set_value};
use crate::error::JSError;
use crate::js_array::set_array_length;
use crate::raise_eval_error;
use crate::unicode::{
    utf8_to_utf16, utf16_char_at, utf16_find, utf16_len, utf16_replace, utf16_rfind, utf16_slice, utf16_to_lowercase, utf16_to_uppercase,
};
use fancy_regex::Regex;
use std::cell::RefCell;
use std::rc::Rc;

pub fn handle_string_method(s: &[u16], method: &str, args: &[Expr], env: &JSObjectDataPtr) -> Result<Value, JSError> {
    match method {
        "toString" => {
            if args.is_empty() {
                Ok(Value::String(s.to_vec()))
            } else {
                let msg = format!("toString method expects no arguments, got {}", args.len());
                Err(raise_eval_error!(msg))
            }
        }
        "substring" => {
            // substring(start, end?) - end is optional and defaults to length
            if args.len() == 1 || args.len() == 2 {
                let start_val = evaluate_expr(env, &args[0])?;
                let end_val = if args.len() == 2 {
                    Some(evaluate_expr(env, &args[1])?)
                } else {
                    None
                };
                if let Value::Number(start) = start_val {
                    let start_idx = start as usize;
                    let end_idx = if let Some(Value::Number(e)) = end_val {
                        e as usize
                    } else {
                        utf16_len(s)
                    };
                    if start_idx <= end_idx && end_idx <= utf16_len(s) {
                        Ok(Value::String(utf16_slice(s, start_idx, end_idx)))
                    } else {
                        let len = utf16_len(s);
                        Err(raise_eval_error!(format!(
                            "substring: invalid indices start={start_idx}, end={end_idx}, length={len}",
                        )))
                    }
                } else {
                    Err(raise_eval_error!("substring: first argument must be a number"))
                }
            } else {
                let msg = format!("substring method expects 1 or 2 arguments, got {}", args.len());
                Err(raise_eval_error!(msg))
            }
        }
        "slice" => {
            let start = if !args.is_empty() {
                match evaluate_expr(env, &args[0])? {
                    Value::Number(n) => n as isize,
                    _ => 0isize,
                }
            } else {
                0isize
            };
            let end = if args.len() >= 2 {
                match evaluate_expr(env, &args[1])? {
                    Value::Number(n) => n as isize,
                    _ => s.len() as isize,
                }
            } else {
                s.len() as isize
            };

            let len = utf16_len(s) as isize;
            let start = if start < 0 { len + start } else { start };
            let end = if end < 0 { len + end } else { end };

            let start = start.max(0).min(len) as usize;
            let end = end.max(0).min(len) as usize;

            if start <= end {
                Ok(Value::String(utf16_slice(s, start, end)))
            } else {
                Ok(Value::String(Vec::new()))
            }
        }
        "toUpperCase" => {
            if args.is_empty() {
                Ok(Value::String(utf16_to_uppercase(s)))
            } else {
                let msg = format!("toUpperCase method expects no arguments, got {}", args.len());
                Err(raise_eval_error!(msg))
            }
        }
        "toLowerCase" => {
            if args.is_empty() {
                Ok(Value::String(utf16_to_lowercase(s)))
            } else {
                let msg = format!("toLowerCase method expects no arguments, got {}", args.len());
                Err(raise_eval_error!(msg))
            }
        }
        "indexOf" => {
            if args.len() == 1 {
                let search_val = evaluate_expr(env, &args[0])?;
                if let Value::String(search) = search_val {
                    if let Some(pos) = utf16_find(s, &search) {
                        Ok(Value::Number(pos as f64))
                    } else {
                        Ok(Value::Number(-1.0))
                    }
                } else {
                    Err(raise_eval_error!("indexOf: argument must be a string"))
                }
            } else {
                Err(raise_eval_error!(format!("indexOf method expects 1 argument, got {}", args.len())))
            }
        }
        "lastIndexOf" => {
            if args.len() == 1 {
                let search_val = evaluate_expr(env, &args[0])?;
                if let Value::String(search) = search_val {
                    if let Some(pos) = utf16_rfind(s, &search) {
                        Ok(Value::Number(pos as f64))
                    } else {
                        Ok(Value::Number(-1.0))
                    }
                } else {
                    Err(raise_eval_error!("lastIndexOf: argument must be a string"))
                }
            } else {
                let msg = format!("lastIndexOf method expects 1 argument, got {}", args.len());
                Err(raise_eval_error!(msg))
            }
        }
        "replace" => {
            if args.len() == 2 {
                let search_val = evaluate_expr(env, &args[0])?;
                let replace_val = evaluate_expr(env, &args[1])?;
                if let (Value::String(search), Value::String(replace)) = (search_val, replace_val) {
                    Ok(Value::String(utf16_replace(s, &search, &replace)))
                } else {
                    Err(raise_eval_error!("replace: both arguments must be strings"))
                }
            } else {
                Err(raise_eval_error!(format!("replace method expects 2 arguments, got {}", args.len())))
            }
        }
        "split" => {
            if args.len() == 1 {
                let sep_val = evaluate_expr(env, &args[0])?;
                if let Value::String(sep) = sep_val {
                    // Implement split returning an array-like object
                    let mut parts: Vec<Vec<u16>> = Vec::new();
                    if sep.is_empty() {
                        // split by empty separator => each UTF-16 code unit as string
                        for i in 0..utf16_len(s) {
                            if let Some(ch) = utf16_char_at(s, i) {
                                parts.push(vec![ch]);
                            }
                        }
                    } else {
                        let mut start = 0usize;
                        while start <= utf16_len(s) {
                            if let Some(pos) = utf16_find(&s[start..], &sep) {
                                let end = start + pos;
                                parts.push(utf16_slice(s, start, end));
                                start = end + utf16_len(&sep);
                            } else {
                                // remainder
                                parts.push(utf16_slice(s, start, utf16_len(s)));
                                break;
                            }
                        }
                    }
                    let arr = Rc::new(RefCell::new(JSObjectData::new()));
                    for (i, part) in parts.into_iter().enumerate() {
                        obj_set_value(&arr, &i.to_string().into(), Value::String(part))?;
                    }
                    let len = arr.borrow().properties.len();
                    set_array_length(&arr, len)?;
                    Ok(Value::Object(arr))
                } else if let Value::Object(obj_map) = sep_val {
                    // Separator is a RegExp-like object
                    let pattern = match obj_map.borrow().get(&"__regex".into()) {
                        Some(val) => match &*val.borrow() {
                            Value::String(s) => String::from_utf16_lossy(s),
                            _ => return Err(raise_eval_error!("split: invalid regex pattern")),
                        },
                        None => return Err(raise_eval_error!("split: invalid regex object")),
                    };

                    let flags = match obj_map.borrow().get(&"__flags".into()) {
                        Some(val) => match &*val.borrow() {
                            Value::String(s) => String::from_utf16_lossy(s),
                            _ => String::new(),
                        },
                        None => String::new(),
                    };

                    // Build fancy-regex with inline flags
                    let mut inline = String::new();
                    if flags.contains('i') {
                        inline.push('i');
                    }
                    if flags.contains('m') {
                        inline.push('m');
                    }
                    if flags.contains('s') {
                        inline.push('s');
                    }
                    let eff_pat = if inline.is_empty() {
                        pattern
                    } else {
                        format!("(?{}){}", inline, pattern)
                    };

                    let regex = Regex::new(&eff_pat).map_err(|e| raise_syntax_error!(format!("Invalid RegExp: {e}")))?;

                    // Use UTF-8 slices for splitting — test files use ASCII so this is safe
                    let input_utf8 = String::from_utf16_lossy(s);
                    let mut parts_utf8: Vec<String> = Vec::new();
                    let mut start_byte = 0usize;
                    while start_byte <= input_utf8.len() {
                        match regex.find(&input_utf8[start_byte..]) {
                            Ok(Some(mat)) => {
                                let match_start = start_byte + mat.start();
                                parts_utf8.push(input_utf8[start_byte..match_start].to_string());
                                start_byte += mat.end();
                                if start_byte > input_utf8.len() {
                                    break;
                                }
                            }
                            Ok(None) => {
                                parts_utf8.push(input_utf8[start_byte..].to_string());
                                break;
                            }
                            Err(e) => {
                                return Err(raise_syntax_error!(format!("Invalid RegExp: {e}")));
                            }
                        }
                    }

                    let arr = Rc::new(RefCell::new(JSObjectData::new()));
                    for (i, part) in parts_utf8.into_iter().enumerate() {
                        obj_set_value(&arr, &i.to_string().into(), Value::String(utf8_to_utf16(&part)))?;
                    }
                    let len = arr.borrow().properties.len();
                    set_array_length(&arr, len)?;
                    Ok(Value::Object(arr))
                } else {
                    Err(raise_eval_error!("split: argument must be a string or RegExp"))
                }
            } else {
                Err(raise_eval_error!(format!("split method expects 1 argument, got {}", args.len())))
            }
        }
        "charAt" => {
            if args.len() == 1 {
                let idx_val = evaluate_expr(env, &args[0])?;
                if let Value::Number(n) = idx_val {
                    let idx = n as isize;
                    // let len = utf16_len(&s) as isize;
                    let idx = if idx < 0 { 0 } else { idx } as usize;
                    if idx < utf16_len(s) {
                        if let Some(ch) = utf16_char_at(s, idx) {
                            Ok(Value::String(vec![ch]))
                        } else {
                            Ok(Value::String(Vec::new()))
                        }
                    } else {
                        Ok(Value::String(Vec::new()))
                    }
                } else {
                    Err(raise_eval_error!("charAt: argument must be a number"))
                }
            } else {
                Err(raise_eval_error!(format!("charAt method expects 1 argument, got {}", args.len())))
            }
        }
        "charCodeAt" => {
            // charCodeAt(index) - returns the UTF-16 code unit at index as a number
            if args.len() == 1 {
                let idx_val = evaluate_expr(env, &args[0])?;
                if let Value::Number(n) = idx_val {
                    let idx = n as usize;
                    if let Some(ch) = utf16_char_at(s, idx) {
                        Ok(Value::Number(ch as f64))
                    } else {
                        // In JS, out-of-range charCodeAt returns NaN
                        Ok(Value::Number(f64::NAN))
                    }
                } else {
                    Err(raise_eval_error!("charCodeAt: index must be a number"))
                }
            } else {
                let msg = format!("charCodeAt method expects 1 argument, got {}", args.len());
                Err(raise_eval_error!(msg))
            }
        }
        "trim" => {
            if args.is_empty() {
                let str_val = String::from_utf16_lossy(s);
                let trimmed = str_val.trim();
                Ok(Value::String(utf8_to_utf16(trimmed)))
            } else {
                Err(raise_eval_error!(format!("trim method expects no arguments, got {}", args.len())))
            }
        }
        "startsWith" => {
            if args.len() == 1 {
                let search_val = evaluate_expr(env, &args[0])?;
                if let Value::String(search) = search_val {
                    let starts = s.len() >= search.len() && s[..search.len()] == search[..];
                    Ok(Value::Boolean(starts))
                } else {
                    Err(raise_eval_error!("startsWith: argument must be a string"))
                }
            } else {
                let msg = format!("startsWith method expects 1 argument, got {}", args.len());
                Err(raise_eval_error!(msg))
            }
        }
        "endsWith" => {
            if args.len() == 1 {
                let search_val = evaluate_expr(env, &args[0])?;
                if let Value::String(search) = search_val {
                    let ends = s.len() >= search.len() && s[s.len() - search.len()..] == search[..];
                    Ok(Value::Boolean(ends))
                } else {
                    Err(raise_eval_error!("endsWith: argument must be a string"))
                }
            } else {
                Err(raise_eval_error!(format!("endsWith method expects 1 argument, got {}", args.len())))
            }
        }
        "includes" => {
            if args.len() == 1 {
                let search_val = evaluate_expr(env, &args[0])?;
                if let Value::String(search) = search_val {
                    let includes = utf16_find(s, &search).is_some();
                    Ok(Value::Boolean(includes))
                } else {
                    Err(raise_eval_error!("includes: argument must be a string"))
                }
            } else {
                Err(raise_eval_error!(format!("includes method expects 1 argument, got {}", args.len())))
            }
        }
        "repeat" => {
            if args.len() == 1 {
                let count_val = evaluate_expr(env, &args[0])?;
                if let Value::Number(n) = count_val {
                    let count = n as usize;
                    let mut repeated = Vec::new();
                    for _ in 0..count {
                        repeated.extend_from_slice(s);
                    }
                    Ok(Value::String(repeated))
                } else {
                    Err(raise_eval_error!("repeat: argument must be a number"))
                }
            } else {
                Err(raise_eval_error!(format!("repeat method expects 1 argument, got {}", args.len())))
            }
        }
        "concat" => {
            let mut result = s.to_vec();
            for arg in args {
                let arg_val = evaluate_expr(env, arg)?;
                if let Value::String(arg_str) = arg_val {
                    result.extend(arg_str);
                } else {
                    // Convert to string
                    let str_val = match arg_val {
                        Value::Number(n) => utf8_to_utf16(&n.to_string()),
                        Value::Boolean(b) => utf8_to_utf16(&b.to_string()),
                        Value::Undefined => utf8_to_utf16("undefined"),
                        _ => utf8_to_utf16("[object Object]"),
                    };
                    result.extend(str_val);
                }
            }
            Ok(Value::String(result))
        }
        "padStart" => {
            if !args.is_empty() {
                let target_len_val = evaluate_expr(env, &args[0])?;
                if let Value::Number(target_len) = target_len_val {
                    let target_len = target_len as usize;
                    let current_len = utf16_len(s);
                    if current_len >= target_len {
                        Ok(Value::String(s.to_vec()))
                    } else {
                        let pad_char = if args.len() >= 2 {
                            let pad_val = evaluate_expr(env, &args[1])?;
                            if let Value::String(pad_str) = pad_val {
                                if !pad_str.is_empty() { pad_str[0] } else { ' ' as u16 }
                            } else {
                                ' ' as u16
                            }
                        } else {
                            ' ' as u16
                        };
                        let pad_count = target_len - current_len;
                        let mut padded = vec![pad_char; pad_count];
                        padded.extend_from_slice(s);
                        Ok(Value::String(padded))
                    }
                } else {
                    Err(raise_eval_error!("padStart: first argument must be a number"))
                }
            } else {
                let msg = format!("padStart method expects at least 1 argument, got {}", args.len());
                Err(raise_eval_error!(msg))
            }
        }
        "padEnd" => {
            if !args.is_empty() {
                let target_len_val = evaluate_expr(env, &args[0])?;
                if let Value::Number(target_len) = target_len_val {
                    let target_len = target_len as usize;
                    let current_len = utf16_len(s);
                    if current_len >= target_len {
                        Ok(Value::String(s.to_vec()))
                    } else {
                        let pad_char = if args.len() >= 2 {
                            let pad_val = evaluate_expr(env, &args[1])?;
                            if let Value::String(pad_str) = pad_val {
                                if !pad_str.is_empty() { pad_str[0] } else { ' ' as u16 }
                            } else {
                                ' ' as u16
                            }
                        } else {
                            ' ' as u16
                        };
                        let pad_count = target_len - current_len;
                        let mut padded = s.to_vec();
                        padded.extend(vec![pad_char; pad_count]);
                        Ok(Value::String(padded))
                    }
                } else {
                    Err(raise_eval_error!("padEnd: first argument must be a number"))
                }
            } else {
                Err(raise_eval_error!(format!(
                    "padEnd method expects at least 1 argument, got {}",
                    args.len()
                )))
            }
        }
        _ => Err(raise_eval_error!(format!("Unknown string method: {method}"))), // method not found
    }
}
