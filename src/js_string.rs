use crate::core::{Expr, JSObjectDataPtr, Value, evaluate_expr, get_own_property, new_js_object_data, obj_set_value};
use crate::error::JSError;
use crate::js_array::set_array_length;
use crate::raise_eval_error;
use crate::unicode::{
    utf8_to_utf16, utf16_char_at, utf16_find, utf16_len, utf16_replace, utf16_rfind, utf16_slice, utf16_to_lowercase, utf16_to_uppercase,
};
use fancy_regex::Regex;

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
                    let mut start_idx = start as isize;
                    let mut end_idx = if let Some(Value::Number(e)) = end_val {
                        e as isize
                    } else {
                        utf16_len(s) as isize
                    };
                    // Handle negative indices: treat as 0
                    if start_idx < 0 {
                        start_idx = 0;
                    }
                    if end_idx < 0 {
                        end_idx = 0;
                    }
                    // Swap if start > end
                    if start_idx > end_idx {
                        std::mem::swap(&mut start_idx, &mut end_idx);
                    }
                    let start_idx = start_idx as usize;
                    let end_idx = end_idx as usize;
                    // Ensure within bounds
                    let len = utf16_len(s);
                    let start_idx = start_idx.min(len);
                    let end_idx = end_idx.min(len);
                    Ok(Value::String(utf16_slice(s, start_idx, end_idx)))
                } else {
                    Err(raise_eval_error!("substring: first argument must be a number"))
                }
            } else {
                let msg = format!("substring method expects 1 or 2 arguments, got {}", args.len());
                Err(raise_eval_error!(msg))
            }
        }
        "substr" => {
            // substr(start, length?) - length is optional and defaults to remaining length
            if args.len() == 1 || args.len() == 2 {
                let start_val = evaluate_expr(env, &args[0])?;
                let length_val = if args.len() == 2 {
                    Some(evaluate_expr(env, &args[1])?)
                } else {
                    None
                };
                if let Value::Number(start) = start_val {
                    let len = utf16_len(s) as isize;
                    let mut start_idx = start as isize;
                    // Handle negative start: count from end
                    if start_idx < 0 {
                        start_idx += len;
                        if start_idx < 0 {
                            start_idx = 0;
                        }
                    }
                    let length = if let Some(Value::Number(l)) = length_val {
                        if l < 0.0 { 0 } else { l as usize }
                    } else {
                        (len - start_idx) as usize
                    };
                    let end_idx = (start_idx + length as isize).min(len);
                    let start_idx = start_idx.max(0) as usize;
                    let end_idx = end_idx.max(0) as usize;
                    Ok(Value::String(utf16_slice(s, start_idx, end_idx)))
                } else {
                    Err(raise_eval_error!("substr: first argument must be a number"))
                }
            } else {
                let msg = format!("substr method expects 1 or 2 arguments, got {}", args.len());
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
            if args.len() == 1 || args.len() == 2 {
                let search_val = evaluate_expr(env, &args[0])?;
                let from_index = if args.len() == 2 {
                    let idx_val = evaluate_expr(env, &args[1])?;
                    if let Value::Number(n) = idx_val { n as isize } else { 0 }
                } else {
                    0
                };
                if let Value::String(search) = search_val {
                    let len = utf16_len(s) as isize;
                    let start = if from_index < 0 { 0 } else { from_index as usize };
                    if start >= len as usize {
                        Ok(Value::Number(-1.0))
                    } else {
                        let slice = &s[start..];
                        if let Some(pos) = utf16_find(slice, &search) {
                            Ok(Value::Number((start + pos) as f64))
                        } else {
                            Ok(Value::Number(-1.0))
                        }
                    }
                } else {
                    Err(raise_eval_error!("indexOf: first argument must be a string"))
                }
            } else {
                Err(raise_eval_error!(format!(
                    "indexOf method expects 1 or 2 arguments, got {}",
                    args.len()
                )))
            }
        }
        "lastIndexOf" => {
            if args.len() == 1 || args.len() == 2 {
                let search_val = evaluate_expr(env, &args[0])?;
                let from_index = if args.len() == 2 {
                    let idx_val = evaluate_expr(env, &args[1])?;
                    if let Value::Number(n) = idx_val {
                        n as isize
                    } else {
                        utf16_len(s) as isize
                    }
                } else {
                    utf16_len(s) as isize
                };
                if let Value::String(search) = search_val {
                    let len = utf16_len(s) as isize;
                    let start = if from_index < 0 {
                        0
                    } else if from_index > len {
                        len
                    } else {
                        from_index
                    };
                    if start <= 0 {
                        Ok(Value::Number(-1.0))
                    } else {
                        let slice = &s[0..start as usize];
                        if let Some(pos) = utf16_rfind(slice, &search) {
                            Ok(Value::Number(pos as f64))
                        } else {
                            Ok(Value::Number(-1.0))
                        }
                    }
                } else {
                    Err(raise_eval_error!("lastIndexOf: first argument must be a string"))
                }
            } else {
                let msg = format!("lastIndexOf method expects 1 or 2 arguments, got {}", args.len());
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
                    let arr = new_js_object_data();
                    for (i, part) in parts.into_iter().enumerate() {
                        obj_set_value(&arr, &i.to_string().into(), Value::String(part))?;
                    }
                    let len = arr.borrow().properties.len();
                    set_array_length(&arr, len)?;
                    Ok(Value::Object(arr))
                } else if let Value::Object(obj_map) = sep_val {
                    // Separator is a RegExp-like object
                    let pattern_opt = get_own_property(&obj_map, &"__regex".into());
                    let pattern = match pattern_opt {
                        Some(val_rc) => match &*val_rc.borrow() {
                            Value::String(s) => String::from_utf16_lossy(s),
                            _ => return Err(raise_eval_error!("split: invalid regex pattern")),
                        },
                        None => return Err(raise_eval_error!("split: invalid regex object")),
                    };

                    let flags_opt = get_own_property(&obj_map, &"__flags".into());
                    let flags = match flags_opt {
                        Some(val_rc) => match &*val_rc.borrow() {
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

                    let arr = new_js_object_data();
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
                    if idx < 0 {
                        Ok(Value::String(Vec::new()))
                    } else {
                        let idx = idx as usize;
                        if idx < utf16_len(s) {
                            if let Some(ch) = utf16_char_at(s, idx) {
                                Ok(Value::String(vec![ch]))
                            } else {
                                Ok(Value::String(Vec::new()))
                            }
                        } else {
                            Ok(Value::String(Vec::new()))
                        }
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
        "trimEnd" => {
            if args.is_empty() {
                let str_val = String::from_utf16_lossy(s);
                let trimmed = str_val.trim_end();
                Ok(Value::String(utf8_to_utf16(trimmed)))
            } else {
                let msg = format!("trimEnd method expects no arguments, got {}", args.len());
                Err(raise_eval_error!(msg))
            }
        }
        "trimStart" => {
            if args.is_empty() {
                let str_val = String::from_utf16_lossy(s);
                let trimmed = str_val.trim_start();
                Ok(Value::String(utf8_to_utf16(trimmed)))
            } else {
                let msg = format!("trimStart method expects no arguments, got {}", args.len());
                Err(raise_eval_error!(msg))
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
