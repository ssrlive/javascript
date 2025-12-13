#![allow(clippy::collapsible_if, clippy::collapsible_match)]

use crate::core::{
    Expr, JSObjectDataPtr, Value, env_get, evaluate_expr, get_own_property, new_js_object_data, obj_get_key_value, obj_set_key_value,
};
use crate::error::JSError;
use crate::js_array::set_array_length;
use crate::js_regexp::{
    RegexKind, get_regex_pattern, handle_regexp_constructor, handle_regexp_method, is_regex_object, sanitize_js_pattern,
};
use crate::unicode::{
    utf8_to_utf16, utf16_char_at, utf16_find, utf16_len, utf16_replace, utf16_rfind, utf16_slice, utf16_to_lowercase, utf16_to_uppercase,
    utf16_to_utf8,
};
use fancy_regex::Regex as FancyRegex;
use regex::Regex as StdRegex;

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
        "substring" => string_substring_method(s, args, env),
        "substr" => string_substr_method(s, args, env),
        "slice" => string_slice_method(s, args, env),
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
        "indexOf" => string_indexof_method(s, args, env),
        "lastIndexOf" => string_lastindexof_method(s, args, env),
        "replace" => string_replace_method(s, args, env),
        "split" => string_split_method(s, args, env),
        "match" => string_match_method(s, args, env),
        "charAt" => string_charat_method(s, args, env),
        "charCodeAt" => string_char_code_at_method(s, args, env),
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

fn string_substring_method(s: &[u16], args: &[Expr], env: &JSObjectDataPtr) -> Result<Value, JSError> {
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

fn string_substr_method(s: &[u16], args: &[Expr], env: &JSObjectDataPtr) -> Result<Value, JSError> {
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

fn string_slice_method(s: &[u16], args: &[Expr], env: &JSObjectDataPtr) -> Result<Value, JSError> {
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

fn string_indexof_method(s: &[u16], args: &[Expr], env: &JSObjectDataPtr) -> Result<Value, JSError> {
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

fn string_lastindexof_method(s: &[u16], args: &[Expr], env: &JSObjectDataPtr) -> Result<Value, JSError> {
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

fn string_replace_method(s: &[u16], args: &[Expr], env: &JSObjectDataPtr) -> Result<Value, JSError> {
    if args.len() == 2 {
        let search_val = evaluate_expr(env, &args[0])?;
        let replace_val = evaluate_expr(env, &args[1])?;
        // If search is a RegExp object, process accordingly
        if let Value::Object(obj_map) = search_val {
            if is_regex_object(&obj_map) {
                // get flags
                let flags = match get_own_property(&obj_map, &"__flags".into()) {
                    Some(val) => match &*val.borrow() {
                        Value::String(s) => utf16_to_utf8(s),
                        _ => "".to_string(),
                    },
                    None => "".to_string(),
                };
                let global = flags.contains('g');

                // Extract pattern to build effective pattern
                let pattern = get_regex_pattern(&obj_map)?;

                // build regex_kind
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
                    pattern.clone()
                } else {
                    format!("(?{}){}", inline, pattern)
                };
                let eff_pat = sanitize_js_pattern(&eff_pat);
                let regex_kind = match FancyRegex::new(&eff_pat) {
                    Ok(r) => RegexKind::Fancy(r),
                    Err(_) => {
                        let sr = StdRegex::new(&eff_pat).map_err(|e| raise_syntax_error!(format!("Invalid RegExp: {e}")))?;
                        RegexKind::Std(sr)
                    }
                };

                // replacement string must be string (function replacement not supported yet)
                if let Value::String(repl_u16) = replace_val {
                    let repl = utf16_to_utf8(&repl_u16);
                    let input_utf8 = String::from_utf16_lossy(s);
                    let mut out = String::new();
                    let mut last_pos = 0usize;

                    // helper to expand replacement tokens ($& only)
                    fn expand_replacement(repl: &str, matched: &str) -> String {
                        let mut out = String::new();
                        let mut chars = repl.chars().peekable();
                        while let Some(ch) = chars.next() {
                            if ch == '$' {
                                if let Some(&next) = chars.peek() {
                                    if next == '&' {
                                        chars.next();
                                        out.push_str(matched);
                                        continue;
                                    }
                                }
                                out.push('$');
                            } else {
                                out.push(ch);
                            }
                        }
                        out
                    }

                    // depending on regex_kind, find matches
                    match regex_kind {
                        RegexKind::Fancy(r) => {
                            let mut search_slice = &input_utf8[..];
                            while let Ok(Some(mat)) = r.find(search_slice) {
                                let start = input_utf8.len() - search_slice.len() + mat.start();
                                let end = input_utf8.len() - search_slice.len() + mat.end();
                                out.push_str(&input_utf8[last_pos..start]);
                                out.push_str(&expand_replacement(&repl, &input_utf8[start..end]));
                                last_pos = end;
                                if !global {
                                    break;
                                }
                                search_slice = &input_utf8[end..];
                            }
                        }
                        RegexKind::Std(r) => {
                            let mut offset = 0usize;
                            while let Some(mat) = r.find(&input_utf8[offset..]) {
                                let start = offset + mat.start();
                                let end = offset + mat.end();
                                out.push_str(&input_utf8[last_pos..start]);
                                out.push_str(&expand_replacement(&repl, &input_utf8[start..end]));
                                last_pos = end;
                                if !global {
                                    break;
                                }
                                offset = end;
                            }
                        }
                    }
                    out.push_str(&input_utf8[last_pos..]);
                    Ok(Value::String(utf8_to_utf16(&out)))
                } else {
                    Err(raise_eval_error!(
                        "replace only supports string as replacement argument for RegExp search"
                    ))
                }
            } else {
                Err(raise_eval_error!("replace: search argument must be a string or RegExp"))
            }
        } else if let (Value::String(search), Value::String(replace)) = (search_val, replace_val) {
            Ok(Value::String(utf16_replace(s, &search, &replace)))
        } else {
            Err(raise_eval_error!("replace: both arguments must be strings"))
        }
    } else {
        Err(raise_eval_error!(format!("replace method expects 2 arguments, got {}", args.len())))
    }
}

fn string_split_method(s: &[u16], args: &[Expr], env: &JSObjectDataPtr) -> Result<Value, JSError> {
    if args.is_empty() || args.len() == 1 || args.len() == 2 {
        let sep_val = if args.is_empty() {
            Value::Undefined
        } else {
            evaluate_expr(env, &args[0])?
        };
        let limit = if args.len() == 2 {
            let limit_val = evaluate_expr(env, &args[1])?;
            if let Value::Number(n) = limit_val {
                if n < 0.0 { usize::MAX } else { n as usize }
            } else {
                usize::MAX
            }
        } else {
            usize::MAX
        };
        if let Value::Undefined = sep_val {
            // No separator: return array with the whole string
            let arr = new_js_object_data();
            let array_result = env_get(env, "Array");
            if let Some(array_val) = &array_result {
                if let Value::Object(array_obj) = &*array_val.borrow() {
                    if let Ok(Some(proto_val)) = obj_get_key_value(array_obj, &"prototype".into()) {
                        if let Value::Object(proto_obj) = &*proto_val.borrow() {
                            arr.borrow_mut().prototype = Some(proto_obj.clone());
                        }
                    }
                }
            }
            obj_set_key_value(&arr, &"0".into(), Value::String(s.to_vec()))?;
            set_array_length(&arr, 1)?;
            Ok(Value::Object(arr))
        } else if let Value::String(sep) = sep_val {
            // Implement split returning an array-like object
            let mut parts: Vec<Vec<u16>> = Vec::new();
            if sep.is_empty() {
                // split by empty separator => each UTF-16 code unit as string
                let len = utf16_len(s).min(limit);
                for i in 0..len {
                    if let Some(ch) = utf16_char_at(s, i) {
                        parts.push(vec![ch]);
                    }
                }
            } else {
                let mut start = 0usize;
                while parts.len() < limit {
                    if let Some(pos) = utf16_find(&s[start..], &sep) {
                        if parts.len() == limit - 1 {
                            if pos == 0 {
                                parts.push(vec![]);
                            } else {
                                parts.push(utf16_slice(s, start, utf16_len(s)));
                            }
                            break;
                        }
                        if pos == 0 {
                            parts.push(vec![]);
                            start += utf16_len(&sep);
                        } else {
                            parts.push(utf16_slice(s, start, start + pos));
                            start += pos + utf16_len(&sep);
                        }
                    } else {
                        parts.push(utf16_slice(s, start, utf16_len(s)));
                        break;
                    }
                }
            }
            let arr = new_js_object_data();
            if let Some(array_val) = env_get(env, "Array") {
                if let Value::Object(array_obj) = &*array_val.borrow() {
                    if let Ok(Some(proto_val)) = obj_get_key_value(array_obj, &"prototype".into()) {
                        if let Value::Object(proto_obj) = &*proto_val.borrow() {
                            arr.borrow_mut().prototype = Some(proto_obj.clone());
                        }
                    }
                }
            }
            for (i, part) in parts.into_iter().enumerate() {
                obj_set_key_value(&arr, &i.to_string().into(), Value::String(part))?;
            }
            let len = arr.borrow().properties.len();
            set_array_length(&arr, len)?;
            Ok(Value::Object(arr))
        } else if let Value::Object(obj_map) = sep_val {
            // Separator is a RegExp-like object
            let pattern = get_regex_pattern(&obj_map)?;

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

            // Try fancy_regex first, then fall back to StdRegex
            let eff_pat = sanitize_js_pattern(&eff_pat);
            let regex_kind_fancy = FancyRegex::new(&eff_pat);
            let input_utf8 = String::from_utf16_lossy(s);

            match regex_kind_fancy {
                Ok(ref fancy) => {
                    let mut parts_utf8: Vec<String> = Vec::new();
                    let mut start_byte = 0usize;
                    while start_byte <= input_utf8.len() && parts_utf8.len() < limit {
                        match fancy.find(&input_utf8[start_byte..]) {
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

                    // continue with parts_utf8
                    let parts_iter = parts_utf8.into_iter();
                    let arr = new_js_object_data();
                    let mut idx = 0usize;
                    for part in parts_iter {
                        obj_set_key_value(&arr, &idx.to_string().into(), Value::String(utf8_to_utf16(&part)))?;
                        idx += 1;
                    }
                    obj_set_key_value(&arr, &"length".into(), Value::Number(idx as f64))?;
                    Ok(Value::Object(arr))
                }
                Err(_) => {
                    let stdregex = StdRegex::new(&eff_pat).map_err(|e| raise_syntax_error!(format!("Invalid RegExp: {e}")))?;
                    let mut parts_utf8: Vec<String> = Vec::new();
                    let mut start_byte = 0usize;
                    while start_byte <= input_utf8.len() && parts_utf8.len() < limit {
                        match stdregex.find(&input_utf8[start_byte..]) {
                            Some(mat) => {
                                let match_start = start_byte + mat.start();
                                parts_utf8.push(input_utf8[start_byte..match_start].to_string());
                                start_byte += mat.end();
                                if start_byte > input_utf8.len() {
                                    break;
                                }
                            }
                            None => {
                                parts_utf8.push(input_utf8[start_byte..].to_string());
                                break;
                            }
                        }
                    }

                    let parts_iter = parts_utf8.into_iter();
                    let arr = new_js_object_data();
                    let mut idx = 0usize;
                    for part in parts_iter {
                        obj_set_key_value(&arr, &idx.to_string().into(), Value::String(utf8_to_utf16(&part)))?;
                        idx += 1;
                    }
                    obj_set_key_value(&arr, &"length".into(), Value::Number(idx as f64))?;
                    Ok(Value::Object(arr))
                }
            }
            // All paths above return a constructed array result.
            // This code path should be unreachable because both branches returned.
        } else {
            Err(raise_eval_error!("split: argument must be a string, RegExp, or undefined"))
        }
    } else {
        let msg = format!("split method expects 0 to 2 arguments, got {}", args.len());
        Err(raise_eval_error!(msg))
    }
}

fn string_match_method(s: &[u16], args: &[Expr], env: &JSObjectDataPtr) -> Result<Value, JSError> {
    // String.prototype.match(search)
    let search_val = if args.is_empty() {
        Value::Undefined
    } else {
        evaluate_expr(env, &args[0])?
    };

    // Build a RegExp object to work with (either existing object or new one)
    let regexp_obj = if let Value::Object(obj_map) = &search_val {
        if is_regex_object(obj_map) {
            obj_map.clone()
        } else {
            // Not a regex object; create new RegExp from string coercion
            let pattern = match &search_val {
                Value::String(su) => utf16_to_utf8(su),
                _ => crate::core::value_to_string(&search_val),
            };
            match handle_regexp_constructor(&[Expr::StringLit(utf8_to_utf16(&pattern))], env)? {
                Value::Object(o) => o,
                _ => return Err(raise_eval_error!("failed to construct RegExp from argument")),
            }
        }
    } else if let Value::String(su) = &search_val {
        match handle_regexp_constructor(&[Expr::StringLit(su.clone())], env)? {
            Value::Object(o) => o,
            _ => return Err(raise_eval_error!("failed to construct RegExp from string")),
        }
    } else if let Value::Undefined = search_val {
        // new RegExp() default
        match handle_regexp_constructor(&[], env)? {
            Value::Object(o) => o,
            _ => return Err(raise_eval_error!("failed to construct default RegExp")),
        }
    } else if let Value::Number(n) = search_val {
        let pat = n.to_string();
        match handle_regexp_constructor(&[Expr::StringLit(utf8_to_utf16(&pat))], env)? {
            Value::Object(o) => o,
            _ => return Err(raise_eval_error!("failed to construct RegExp from number")),
        }
    } else if let Value::Boolean(b) = search_val {
        let pat = b.to_string();
        match handle_regexp_constructor(&[Expr::StringLit(utf8_to_utf16(&pat))], env)? {
            Value::Object(o) => o,
            _ => return Err(raise_eval_error!("failed to construct RegExp from bool")),
        }
    } else {
        // Fallback: coerce to string using value_to_string
        let pat = crate::core::value_to_string(&search_val);
        match handle_regexp_constructor(&[Expr::StringLit(utf8_to_utf16(&pat))], env)? {
            Value::Object(o) => o,
            _ => return Err(raise_eval_error!("failed to construct RegExp from arg")),
        }
    };

    // Determine flags
    let flags = match get_own_property(&regexp_obj, &"__flags".into()) {
        Some(val) => match &*val.borrow() {
            Value::String(s) => utf16_to_utf8(s),
            _ => String::new(),
        },
        None => String::new(),
    };

    let global = flags.contains('g');

    // Build arg for exec: the string to match
    let exec_arg = Expr::StringLit(s.to_vec());
    let exec_args = vec![exec_arg.clone()];

    if global {
        // Save lastIndex
        let prev_last_index = get_own_property(&regexp_obj, &"__lastIndex".into());
        // Reset lastIndex to 0 for global matching
        obj_set_key_value(&regexp_obj, &"__lastIndex".into(), Value::Number(0.0))?;

        let mut matches: Vec<String> = Vec::new();
        loop {
            match handle_regexp_method(&regexp_obj, "exec", &exec_args, env)? {
                Value::Object(arr) => {
                    if let Some(val_rc) = obj_get_key_value(&arr, &"0".into())? {
                        match &*val_rc.borrow() {
                            Value::String(u16s) => matches.push(utf16_to_utf8(u16s)),
                            _ => matches.push("".to_string()),
                        }
                    } else {
                        // No match value found - stop
                        break;
                    }
                }
                Value::Null => {
                    break;
                }
                _ => {
                    break;
                }
            }
        }

        // Restore lastIndex
        if let Some(val) = prev_last_index {
            obj_set_key_value(&regexp_obj, &"__lastIndex".into(), val.borrow().clone())?;
        } else {
            obj_set_key_value(&regexp_obj, &"__lastIndex".into(), Value::Number(0.0))?;
        }

        if matches.is_empty() {
            return Ok(Value::Null);
        }

        // Convert matches to JS array-like
        let arr = new_js_object_data();
        let array_proto = env_get(env, "Array");
        if let Some(array_val) = &array_proto {
            if let Value::Object(array_obj) = &*array_val.borrow() {
                if let Ok(Some(proto_val)) = obj_get_key_value(array_obj, &"prototype".into()) {
                    if let Value::Object(proto_obj) = &*proto_val.borrow() {
                        arr.borrow_mut().prototype = Some(proto_obj.clone());
                    }
                }
            }
        }
        for (i, m) in matches.iter().enumerate() {
            obj_set_key_value(&arr, &i.to_string().into(), Value::String(utf8_to_utf16(m)))?;
        }
        obj_set_key_value(&arr, &"length".into(), Value::Number(matches.len() as f64))?;
        Ok(Value::Object(arr))
    } else {
        // Non-global: delegate to RegExp.prototype.exec and return result
        let res = handle_regexp_method(&regexp_obj, "exec", &exec_args, env)?;
        Ok(res)
    }
}

fn string_charat_method(s: &[u16], args: &[Expr], env: &JSObjectDataPtr) -> Result<Value, JSError> {
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

fn string_char_code_at_method(s: &[u16], args: &[Expr], env: &JSObjectDataPtr) -> Result<Value, JSError> {
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
