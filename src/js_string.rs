#![allow(clippy::collapsible_if, clippy::collapsible_match)]

use crate::core::{Expr, JSObjectDataPtr, Value, evaluate_expr, get_own_property, obj_get_key_value, obj_set_key_value, to_primitive};
use crate::error::JSError;
use crate::js_array::set_array_length;
use crate::js_regexp::{handle_regexp_constructor, handle_regexp_method, is_regex_object};
use crate::unicode::{
    utf8_to_utf16, utf16_char_at, utf16_find, utf16_len, utf16_replace, utf16_rfind, utf16_slice, utf16_to_lowercase, utf16_to_uppercase,
    utf16_to_utf8,
};

pub(crate) fn string_constructor(args: &[Expr], env: &JSObjectDataPtr) -> Result<Value, JSError> {
    // String() constructor
    if args.len() == 1 {
        let arg_val = evaluate_expr(env, &args[0])?;
        match arg_val {
            Value::Number(n) => Ok(Value::String(utf8_to_utf16(&n.to_string()))),
            Value::String(s) => Ok(Value::String(s.clone())),
            Value::Boolean(b) => Ok(Value::String(utf8_to_utf16(&b.to_string()))),
            Value::Undefined => Ok(Value::String(utf8_to_utf16("undefined"))),
            Value::Null => Ok(Value::String(utf8_to_utf16("null"))),
            Value::Object(obj) => {
                // Attempt ToPrimitive with 'string' hint first (honor [Symbol.toPrimitive] or fallback)
                let prim = to_primitive(&Value::Object(obj.clone()), "string", env)?;
                match prim {
                    Value::String(s) => Ok(Value::String(s)),
                    Value::Number(n) => Ok(Value::String(utf8_to_utf16(&n.to_string()))),
                    Value::Boolean(b) => Ok(Value::String(utf8_to_utf16(&b.to_string()))),
                    Value::Symbol(sd) => match sd.description {
                        Some(ref d) => Ok(Value::String(utf8_to_utf16(&format!("Symbol({})", d)))),
                        None => Ok(Value::String(utf8_to_utf16("Symbol()"))),
                    },
                    _ => Ok(Value::String(utf8_to_utf16("[object Object]"))),
                }
            }
            Value::Function(name) => Ok(Value::String(utf8_to_utf16(&format!("[Function: {name}]")))),
            Value::Closure(_) | Value::AsyncClosure(_) => Ok(Value::String(utf8_to_utf16("[Function]"))),
            Value::ClassDefinition(_) => Ok(Value::String(utf8_to_utf16("[Class]"))),
            Value::Getter(..) => Ok(Value::String(utf8_to_utf16("[Getter]"))),
            Value::Setter(..) => Ok(Value::String(utf8_to_utf16("[Setter]"))),
            Value::Property { .. } => Ok(Value::String(utf8_to_utf16("[property]"))),
            Value::Promise(_) => Ok(Value::String(utf8_to_utf16("[object Promise]"))),
            Value::Symbol(symbol_data) => match &symbol_data.description {
                Some(d) => Ok(Value::String(utf8_to_utf16(&format!("Symbol({d})")))),
                None => Ok(Value::String(utf8_to_utf16("Symbol()"))),
            },
            Value::BigInt(h) => Ok(Value::String(utf8_to_utf16(&h.to_string()))),
            Value::Map(_) => Ok(Value::String(utf8_to_utf16("[object Map]"))),
            Value::Set(_) => Ok(Value::String(utf8_to_utf16("[object Set]"))),
            Value::WeakMap(_) => Ok(Value::String(utf8_to_utf16("[object WeakMap]"))),
            Value::WeakSet(_) => Ok(Value::String(utf8_to_utf16("[object WeakSet]"))),
            Value::GeneratorFunction(..) => Ok(Value::String(utf8_to_utf16("[GeneratorFunction]"))),
            Value::Generator(_) => Ok(Value::String(utf8_to_utf16("[object Generator]"))),
            Value::Proxy(_) => Ok(Value::String(utf8_to_utf16("[object Proxy]"))),
            Value::ArrayBuffer(_) => Ok(Value::String(utf8_to_utf16("[object ArrayBuffer]"))),
            Value::DataView(_) => Ok(Value::String(utf8_to_utf16("[object DataView]"))),
            Value::TypedArray(_) => Ok(Value::String(utf8_to_utf16("[object TypedArray]"))),
            Value::Uninitialized => Ok(Value::String(utf8_to_utf16("undefined"))),
        }
    } else {
        Ok(Value::String(Vec::new())) // String() with no args returns empty string
    }
}

pub fn handle_string_method(s: &[u16], method: &str, args: &[Expr], env: &JSObjectDataPtr) -> Result<Value, JSError> {
    match method {
        "toString" => string_to_string_method(s, args, env),
        "valueOf" => string_to_string_method(s, args, env),
        "substring" => string_substring_method(s, args, env),
        "substr" => string_substr_method(s, args, env),
        "slice" => string_slice_method(s, args, env),
        "toUpperCase" => string_to_uppercase(s, args, env),
        "toLowerCase" => string_to_lowercase(s, args, env),
        "indexOf" => string_indexof_method(s, args, env),
        "lastIndexOf" => string_lastindexof_method(s, args, env),
        "replace" => string_replace_method(s, args, env),
        "split" => string_split_method(s, args, env),
        "match" => string_match_method(s, args, env),
        "charAt" => string_charat_method(s, args, env),
        "charCodeAt" => string_char_code_at_method(s, args, env),
        "trim" => string_trim_method(s, args, env),
        "trimEnd" => string_trim_end_method(s, args, env),
        "trimStart" => string_trim_start_method(s, args, env),
        "startsWith" => string_starts_with_method(s, args, env),
        "endsWith" => string_ends_with_method(s, args, env),
        "includes" => string_includes_method(s, args, env),
        "repeat" => string_repeat_method(s, args, env),
        "concat" => string_concat_method(s, args, env),
        "padStart" => string_pad_start_method(s, args, env),
        "padEnd" => string_pad_end_method(s, args, env),
        "at" => string_at_method(s, args, env),
        "codePointAt" => string_code_point_at_method(s, args, env),
        "search" => string_search_method(s, args, env),
        "matchAll" => string_match_all_method(s, args, env),
        "toLocaleLowerCase" => string_to_locale_lowercase(s, args, env),
        "toLocaleUpperCase" => string_to_locale_uppercase(s, args, env),
        "normalize" => string_normalize_method(s, args, env),
        "toWellFormed" => string_to_well_formed_method(s, args, env),
        "replaceAll" => string_replace_all_method(s, args, env),
        _ => Err(raise_eval_error!(format!("Unknown string method: {method}"))), // method not found
    }
}

fn string_to_string_method(s: &[u16], args: &[Expr], _env: &JSObjectDataPtr) -> Result<Value, JSError> {
    if args.is_empty() {
        Ok(Value::String(s.to_vec()))
    } else {
        let msg = format!("toString method expects no arguments, got {}", args.len());
        Err(raise_eval_error!(msg))
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

fn string_to_uppercase(s: &[u16], args: &[Expr], _env: &JSObjectDataPtr) -> Result<Value, JSError> {
    if args.is_empty() {
        Ok(Value::String(utf16_to_uppercase(s)))
    } else {
        let msg = format!("toUpperCase method expects no arguments, got {}", args.len());
        Err(raise_eval_error!(msg))
    }
}

fn string_to_lowercase(s: &[u16], args: &[Expr], _env: &JSObjectDataPtr) -> Result<Value, JSError> {
    if args.is_empty() {
        Ok(Value::String(utf16_to_lowercase(s)))
    } else {
        let msg = format!("toLowerCase method expects no arguments, got {}", args.len());
        Err(raise_eval_error!(msg))
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
        if let Value::Object(object) = search_val {
            if is_regex_object(&object) {
                // get flags
                let flags = match get_own_property(&object, &"__flags".into()) {
                    Some(val) => match &*val.borrow() {
                        Value::String(s) => utf16_to_utf8(s),
                        _ => "".to_string(),
                    },
                    None => "".to_string(),
                };
                let global = flags.contains('g');

                // Extract pattern
                let pattern_u16 = crate::js_regexp::internal_get_regex_pattern(&object)?;

                let re = crate::js_regexp::create_regex_from_utf16(&pattern_u16, &flags)
                    .map_err(|e| raise_syntax_error!(format!("Invalid RegExp: {}", e)))?;

                // replacement string must be string (function replacement not supported yet)
                if let Value::String(repl_u16) = replace_val {
                    let repl = utf16_to_utf8(&repl_u16);
                    let mut out: Vec<u16> = Vec::new();
                    let mut last_pos = 0usize;

                    // helper to expand replacement tokens ($&, $1, $2, $`, $', $$)
                    fn expand_replacement(
                        repl: &str,
                        matched: &[u16],
                        captures: &[Option<Vec<u16>>],
                        before: &[u16],
                        after: &[u16],
                    ) -> Vec<u16> {
                        let mut out = String::new();
                        let mut chars = repl.chars().peekable();
                        while let Some(ch) = chars.next() {
                            if ch == '$' {
                                if let Some(&next) = chars.peek() {
                                    match next {
                                        '&' => {
                                            chars.next();
                                            out.push_str(&utf16_to_utf8(matched));
                                        }
                                        '`' => {
                                            chars.next();
                                            out.push_str(&utf16_to_utf8(before));
                                        }
                                        '\'' => {
                                            chars.next();
                                            out.push_str(&utf16_to_utf8(after));
                                        }
                                        '$' => {
                                            chars.next();
                                            out.push('$');
                                        }
                                        '0'..='9' => {
                                            // $1, $2, etc.
                                            let mut num_str = String::new();
                                            num_str.push(next);
                                            chars.next();
                                            while let Some(&digit @ '0'..='9') = chars.peek() {
                                                num_str.push(digit);
                                                chars.next();
                                            }
                                            if let Ok(n) = num_str.parse::<usize>() {
                                                if n > 0 && n <= captures.len() {
                                                    if let Some(ref cap) = captures[n - 1] {
                                                        out.push_str(&utf16_to_utf8(cap));
                                                    }
                                                } else {
                                                    // If n is out of bounds, treat as literal?
                                                    // JS spec says: if $n is not a capture, it's literal $n.
                                                    // But here we parsed it.
                                                    // For simplicity, just ignore or push literal.
                                                    out.push('$');
                                                    out.push_str(&num_str);
                                                }
                                            }
                                        }
                                        _ => {
                                            out.push('$');
                                        }
                                    }
                                } else {
                                    out.push('$');
                                }
                            } else {
                                out.push(ch);
                            }
                        }
                        utf8_to_utf16(&out)
                    }

                    let mut offset = 0usize;
                    // regress doesn't have an iterator for matches that handles overlap/global automatically in a simple way?
                    // It has `find_iter` but that might not handle `lastIndex` updates if we were doing that.
                    // But here we just want all matches.
                    // `re.find_iter` returns an iterator.

                    // Wait, `find_iter` is for `&str`. `find_iter_utf16`?
                    // regress 0.4.1 has `find_iter`. Does it support `&[u16]`?
                    // `Regex::find_iter` takes `text`.
                    // Let's check if `regress` has `find_iter` for utf16.
                    // The README says `*_utf16` family.
                    // I'll assume `find_iter_utf16` exists or I loop manually.
                    // Manual loop is safer.

                    while let Some(m) = re.find_from_utf16(s, offset).next() {
                        let start = m.range.start;
                        let end = m.range.end;

                        // If global and zero-length match, we must advance by 1 to avoid infinite loop
                        // But we must also include the zero-length match in replacement.

                        let before = &s[..start];
                        let after = &s[end..];
                        let matched = &s[start..end];

                        let mut captures = Vec::new();
                        for cap in m.captures.iter() {
                            if let Some(range) = cap {
                                captures.push(Some(s[range.start..range.end].to_vec()));
                            } else {
                                captures.push(None);
                            }
                        }

                        out.extend_from_slice(&s[last_pos..start]);
                        out.extend_from_slice(&expand_replacement(&repl, matched, &captures, before, after));
                        last_pos = end;

                        if !global {
                            break;
                        }

                        if start == end {
                            offset = end + 1;
                        } else {
                            offset = end;
                        }
                        if offset > s.len() {
                            break;
                        }
                    }

                    out.extend_from_slice(&s[last_pos..]);
                    Ok(Value::String(out))
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
            let arr = crate::js_array::create_array(env)?;
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
                        parts.push(utf16_slice(s, start, start + pos));
                        start += pos + utf16_len(&sep);
                    } else {
                        parts.push(utf16_slice(s, start, utf16_len(s)));
                        break;
                    }
                }
            }
            let arr = crate::js_array::create_array(env)?;
            for (i, part) in parts.iter().enumerate() {
                obj_set_key_value(&arr, &i.to_string().into(), Value::String(part.clone()))?;
            }
            set_array_length(&arr, parts.len())?;
            Ok(Value::Object(arr))
        } else if let Value::Object(object) = sep_val {
            // Separator is a RegExp-like object
            let pattern_u16 = crate::js_regexp::internal_get_regex_pattern(&object)?;

            let flags_opt = get_own_property(&object, &"__flags".into());
            let flags = match flags_opt {
                Some(val_rc) => match &*val_rc.borrow() {
                    Value::String(s) => utf16_to_utf8(s),
                    _ => String::new(),
                },
                None => String::new(),
            };

            let re = crate::js_regexp::create_regex_from_utf16(&pattern_u16, &flags)
                .map_err(|e| raise_syntax_error!(format!("Invalid RegExp: {}", e)))?;

            let mut parts: Vec<Value> = Vec::new();
            let mut start = 0usize;
            let mut offset = 0usize;

            loop {
                if parts.len() >= limit {
                    break;
                }

                match re.find_from_utf16(s, offset).next() {
                    Some(m) => {
                        let match_start = m.range.start;
                        let match_end = m.range.end;

                        if match_start == match_end && match_start == start {
                            if offset < s.len() {
                                offset += 1;
                                continue;
                            } else {
                                parts.push(Value::String(Vec::new()));
                                break;
                            }
                        }

                        parts.push(Value::String(s[start..match_start].to_vec()));

                        // Capturing groups
                        for cap in m.captures.iter() {
                            if let Some(range) = cap {
                                parts.push(Value::String(s[range.start..range.end].to_vec()));
                            } else {
                                parts.push(Value::Undefined);
                            }
                            if parts.len() >= limit {
                                break;
                            }
                        }

                        start = match_end;
                        offset = match_end;

                        if offset > s.len() {
                            break;
                        }
                    }
                    None => {
                        parts.push(Value::String(s[start..].to_vec()));
                        break;
                    }
                }
            }

            let arr = crate::js_array::create_array(env)?;
            for (i, part) in parts.iter().enumerate() {
                obj_set_key_value(&arr, &i.to_string().into(), part.clone())?;
            }
            set_array_length(&arr, parts.len())?;
            Ok(Value::Object(arr))
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
    let regexp_obj = if let Value::Object(object) = &search_val {
        if is_regex_object(object) {
            object.clone()
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
        // Save lastIndex (prefer user-visible `lastIndex`)
        let prev_last_index = get_own_property(&regexp_obj, &"lastIndex".into());
        // Reset lastIndex to 0 for global matching
        obj_set_key_value(&regexp_obj, &"lastIndex".into(), Value::Number(0.0))?;

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
            obj_set_key_value(&regexp_obj, &"lastIndex".into(), val.borrow().clone())?;
        } else {
            obj_set_key_value(&regexp_obj, &"lastIndex".into(), Value::Number(0.0))?;
        }

        if matches.is_empty() {
            return Ok(Value::Null);
        }

        // Convert matches to JS array-like
        let arr = crate::js_array::create_array(env)?;
        for (i, m) in matches.iter().enumerate() {
            obj_set_key_value(&arr, &i.to_string().into(), Value::String(utf8_to_utf16(m)))?;
        }
        set_array_length(&arr, matches.len())?;
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

fn string_trim_method(s: &[u16], args: &[Expr], _env: &JSObjectDataPtr) -> Result<Value, JSError> {
    if args.is_empty() {
        let str_val = utf16_to_utf8(s);
        let trimmed = str_val.trim();
        Ok(Value::String(utf8_to_utf16(trimmed)))
    } else {
        Err(raise_eval_error!(format!("trim method expects no arguments, got {}", args.len())))
    }
}

fn string_trim_end_method(s: &[u16], args: &[Expr], _env: &JSObjectDataPtr) -> Result<Value, JSError> {
    if args.is_empty() {
        let str_val = utf16_to_utf8(s);
        let trimmed = str_val.trim_end();
        Ok(Value::String(utf8_to_utf16(trimmed)))
    } else {
        let msg = format!("trimEnd method expects no arguments, got {}", args.len());
        Err(raise_eval_error!(msg))
    }
}

fn string_trim_start_method(s: &[u16], args: &[Expr], _env: &JSObjectDataPtr) -> Result<Value, JSError> {
    if args.is_empty() {
        let str_val = utf16_to_utf8(s);
        let trimmed = str_val.trim_start();
        Ok(Value::String(utf8_to_utf16(trimmed)))
    } else {
        let msg = format!("trimStart method expects no arguments, got {}", args.len());
        Err(raise_eval_error!(msg))
    }
}

fn string_starts_with_method(s: &[u16], args: &[Expr], env: &JSObjectDataPtr) -> Result<Value, JSError> {
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

fn string_ends_with_method(s: &[u16], args: &[Expr], env: &JSObjectDataPtr) -> Result<Value, JSError> {
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

fn string_includes_method(s: &[u16], args: &[Expr], env: &JSObjectDataPtr) -> Result<Value, JSError> {
    if args.is_empty() {
        return Err(raise_eval_error!("includes method expects at least 1 argument"));
    }
    let search_val = evaluate_expr(env, &args[0])?;
    let search_str = to_primitive(&search_val, "string", env)?;
    let search = if let Value::String(s) = search_str {
        s
    } else {
        return Err(raise_eval_error!("includes: argument must be a string"));
    };

    let position = if args.len() > 1 {
        let pos_val = evaluate_expr(env, &args[1])?;
        if let Value::Number(n) = pos_val { n as usize } else { 0 }
    } else {
        0
    };

    if position >= s.len() {
        return Ok(Value::Boolean(false));
    }

    let includes = utf16_find(&s[position..], &search).is_some();
    Ok(Value::Boolean(includes))
}

fn string_repeat_method(s: &[u16], args: &[Expr], env: &JSObjectDataPtr) -> Result<Value, JSError> {
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

fn string_concat_method(s: &[u16], args: &[Expr], env: &JSObjectDataPtr) -> Result<Value, JSError> {
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

fn string_pad_start_method(s: &[u16], args: &[Expr], env: &JSObjectDataPtr) -> Result<Value, JSError> {
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

fn string_pad_end_method(s: &[u16], args: &[Expr], env: &JSObjectDataPtr) -> Result<Value, JSError> {
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

fn string_at_method(s: &[u16], args: &[Expr], env: &JSObjectDataPtr) -> Result<Value, JSError> {
    let idx = if !args.is_empty() {
        match evaluate_expr(env, &args[0])? {
            Value::Number(n) => n as i64,
            _ => 0,
        }
    } else {
        0
    };
    let len = s.len() as i64;
    let k = if idx >= 0 { idx } else { len + idx };
    if k < 0 || k >= len {
        Ok(Value::Undefined)
    } else {
        Ok(Value::String(vec![s[k as usize]]))
    }
}

fn string_code_point_at_method(s: &[u16], args: &[Expr], env: &JSObjectDataPtr) -> Result<Value, JSError> {
    let idx = if !args.is_empty() {
        match evaluate_expr(env, &args[0])? {
            Value::Number(n) => n as usize,
            _ => 0,
        }
    } else {
        0
    };
    if idx >= s.len() {
        return Ok(Value::Undefined);
    }
    let first = s[idx];
    if (0xD800..=0xDBFF).contains(&first) && idx + 1 < s.len() {
        let second = s[idx + 1];
        if (0xDC00..=0xDFFF).contains(&second) {
            let code_point = 0x10000 + ((first as u32 - 0xD800) << 10) + (second as u32 - 0xDC00);
            return Ok(Value::Number(code_point as f64));
        }
    }
    Ok(Value::Number(first as f64))
}

fn string_search_method(s: &[u16], args: &[Expr], env: &JSObjectDataPtr) -> Result<Value, JSError> {
    let (regexp_obj, _flags) = if !args.is_empty() {
        let arg = evaluate_expr(env, &args[0])?;
        match arg {
            Value::Object(obj) if is_regex_object(&obj) => {
                let _p = crate::js_regexp::internal_get_regex_pattern(&obj)?;
                let f = match get_own_property(&obj, &"__flags".into()) {
                    Some(val) => match &*val.borrow() {
                        Value::String(s) => utf16_to_utf8(s),
                        _ => String::new(),
                    },
                    None => String::new(),
                };
                (obj, f)
            }
            Value::String(p) => {
                let re_args = vec![Expr::StringLit(p.clone())];
                let val = handle_regexp_constructor(&re_args, env)?;
                if let Value::Object(obj) = val {
                    (obj, String::new())
                } else {
                    return Err(raise_eval_error!("Failed to create RegExp"));
                }
            }
            v => {
                let p = utf8_to_utf16(&v.to_string());
                let re_args = vec![Expr::StringLit(p)];
                let val = handle_regexp_constructor(&re_args, env)?;
                if let Value::Object(obj) = val {
                    (obj, String::new())
                } else {
                    return Err(raise_eval_error!("Failed to create RegExp"));
                }
            }
        }
    } else {
        let re_args = vec![Expr::StringLit(Vec::new())];
        let val = handle_regexp_constructor(&re_args, env)?;
        if let Value::Object(obj) = val {
            (obj, String::new())
        } else {
            return Err(raise_eval_error!("Failed to create RegExp"));
        }
    };

    let pattern = crate::js_regexp::internal_get_regex_pattern(&regexp_obj)?;
    let flags_str = match get_own_property(&regexp_obj, &"__flags".into()) {
        Some(val) => match &*val.borrow() {
            Value::String(s) => utf16_to_utf8(s),
            _ => String::new(),
        },
        None => String::new(),
    };

    let re_args = vec![Expr::StringLit(pattern), Expr::StringLit(utf8_to_utf16(&flags_str))];
    let matcher_val = handle_regexp_constructor(&re_args, env)?;
    let matcher_obj = if let Value::Object(o) = matcher_val {
        o
    } else {
        return Err(raise_eval_error!("Failed to clone RegExp"));
    };

    obj_set_key_value(&matcher_obj, &"lastIndex".into(), Value::Number(0.0))?;

    let exec_args = vec![Expr::StringLit(s.to_vec())];
    let res = handle_regexp_method(&matcher_obj, "exec", &exec_args, env)?;

    match res {
        Value::Object(match_obj) => {
            if let Some(idx_val) = obj_get_key_value(&match_obj, &"index".into())? {
                Ok(idx_val.borrow().clone())
            } else {
                Ok(Value::Number(-1.0))
            }
        }
        _ => Ok(Value::Number(-1.0)),
    }
}

fn string_match_all_method(s: &[u16], args: &[Expr], env: &JSObjectDataPtr) -> Result<Value, JSError> {
    let (regexp_obj, flags) = if !args.is_empty() {
        let arg = evaluate_expr(env, &args[0])?;
        match arg {
            Value::Object(obj) if is_regex_object(&obj) => {
                let f = match get_own_property(&obj, &"__flags".into()) {
                    Some(val) => match &*val.borrow() {
                        Value::String(s) => utf16_to_utf8(s),
                        _ => String::new(),
                    },
                    None => String::new(),
                };
                if !f.contains('g') {
                    return Err(raise_type_error!(
                        "String.prototype.matchAll called with a non-global RegExp argument"
                    ));
                }
                (obj, f)
            }
            Value::String(p) => {
                let re_args = vec![Expr::StringLit(p.clone()), Expr::StringLit(utf8_to_utf16("g"))];
                let val = handle_regexp_constructor(&re_args, env)?;
                if let Value::Object(obj) = val {
                    (obj, String::from("g"))
                } else {
                    return Err(raise_eval_error!("Failed to create RegExp"));
                }
            }
            _ => {
                let arg_val = evaluate_expr(env, &args[0])?;
                let p = match arg_val {
                    Value::String(s) => s,
                    v => utf8_to_utf16(&v.to_string()),
                };
                let re_args = vec![Expr::StringLit(p), Expr::StringLit(utf8_to_utf16("g"))];
                let val = handle_regexp_constructor(&re_args, env)?;
                if let Value::Object(obj) = val {
                    (obj, String::from("g"))
                } else {
                    return Err(raise_eval_error!("Failed to create RegExp"));
                }
            }
        }
    } else {
        let re_args = vec![Expr::StringLit(Vec::new()), Expr::StringLit(utf8_to_utf16("g"))];
        let val = handle_regexp_constructor(&re_args, env)?;
        if let Value::Object(obj) = val {
            (obj, String::from("g"))
        } else {
            return Err(raise_eval_error!("Failed to create RegExp"));
        }
    };

    let pattern = crate::js_regexp::internal_get_regex_pattern(&regexp_obj)?;
    let flags_u16 = utf8_to_utf16(&flags);
    let re_args = vec![Expr::StringLit(pattern), Expr::StringLit(flags_u16)];
    let matcher_val = handle_regexp_constructor(&re_args, env)?;
    let matcher_obj = if let Value::Object(o) = matcher_val {
        o
    } else {
        return Err(raise_eval_error!("Failed to clone RegExp"));
    };

    obj_set_key_value(&matcher_obj, &"lastIndex".into(), Value::Number(0.0))?;

    let mut matches = Vec::new();
    let exec_args = vec![Expr::StringLit(s.to_vec())];

    loop {
        let res = handle_regexp_method(&matcher_obj, "exec", &exec_args, env)?;
        match res {
            Value::Null => break,
            Value::Object(match_obj) => {
                matches.push(Value::Object(match_obj.clone()));

                if let Some(m0) = obj_get_key_value(&match_obj, &"0".into())? {
                    if let Value::String(s) = &*m0.borrow() {
                        if s.is_empty() {
                            if let Some(li) = obj_get_key_value(&matcher_obj, &"lastIndex".into())? {
                                if let Value::Number(n) = *li.borrow() {
                                    obj_set_key_value(&matcher_obj, &"lastIndex".into(), Value::Number(n + 1.0))?;
                                }
                            }
                        }
                    }
                }
            }
            _ => break,
        }
    }

    make_array_from_values(env, matches)
}

fn string_to_locale_lowercase(s: &[u16], args: &[Expr], env: &JSObjectDataPtr) -> Result<Value, JSError> {
    string_to_lowercase(s, args, env)
}

fn string_to_locale_uppercase(s: &[u16], args: &[Expr], env: &JSObjectDataPtr) -> Result<Value, JSError> {
    string_to_uppercase(s, args, env)
}

fn string_normalize_method(s: &[u16], _args: &[Expr], _env: &JSObjectDataPtr) -> Result<Value, JSError> {
    Ok(Value::String(s.to_vec()))
}

fn string_to_well_formed_method(s: &[u16], _args: &[Expr], _env: &JSObjectDataPtr) -> Result<Value, JSError> {
    let mut res = Vec::with_capacity(s.len());
    let mut i = 0;
    while i < s.len() {
        let c = s[i];
        if (0xD800..=0xDBFF).contains(&c) {
            if i + 1 < s.len() {
                let next = s[i + 1];
                if (0xDC00..=0xDFFF).contains(&next) {
                    res.push(c);
                    res.push(next);
                    i += 2;
                    continue;
                }
            }
            res.push(0xFFFD);
            i += 1;
        } else if (0xDC00..=0xDFFF).contains(&c) {
            res.push(0xFFFD);
            i += 1;
        } else {
            res.push(c);
            i += 1;
        }
    }
    Ok(Value::String(res))
}

fn make_array_from_values(env: &JSObjectDataPtr, values: Vec<Value>) -> Result<Value, JSError> {
    let len = values.len();
    let arr = crate::js_array::create_array(env)?;
    for (i, v) in values.into_iter().enumerate() {
        obj_set_key_value(&arr, &i.to_string().into(), v)?;
    }
    crate::js_array::set_array_length(&arr, len)?;
    Ok(Value::Object(arr))
}

fn string_replace_all_method(s: &[u16], args: &[Expr], env: &JSObjectDataPtr) -> Result<Value, JSError> {
    if args.len() == 2 {
        let search_val = evaluate_expr(env, &args[0])?;
        let replace_val = evaluate_expr(env, &args[1])?;

        if let Value::Object(object) = search_val {
            if is_regex_object(&object) {
                // get flags
                let flags = match get_own_property(&object, &"__flags".into()) {
                    Some(val) => match &*val.borrow() {
                        Value::String(s) => utf16_to_utf8(s),
                        _ => "".to_string(),
                    },
                    None => "".to_string(),
                };
                if !flags.contains('g') {
                    return Err(raise_type_error!(
                        "String.prototype.replaceAll called with a non-global RegExp argument"
                    ));
                }

                // Extract pattern
                let pattern_u16 = crate::js_regexp::internal_get_regex_pattern(&object)?;

                let re = crate::js_regexp::create_regex_from_utf16(&pattern_u16, &flags)
                    .map_err(|e| raise_syntax_error!(format!("Invalid RegExp: {}", e)))?;

                if let Value::String(repl_u16) = replace_val {
                    let repl = utf16_to_utf8(&repl_u16);
                    let mut out: Vec<u16> = Vec::new();
                    let mut last_pos = 0usize;

                    // helper to expand replacement tokens
                    fn expand_replacement(
                        repl: &str,
                        matched: &[u16],
                        captures: &[Option<Vec<u16>>],
                        before: &[u16],
                        after: &[u16],
                    ) -> Vec<u16> {
                        let mut out = String::new();
                        let mut chars = repl.chars().peekable();
                        while let Some(ch) = chars.next() {
                            if ch == '$' {
                                if let Some(&next) = chars.peek() {
                                    match next {
                                        '&' => {
                                            chars.next();
                                            out.push_str(&utf16_to_utf8(matched));
                                        }
                                        '`' => {
                                            chars.next();
                                            out.push_str(&utf16_to_utf8(before));
                                        }
                                        '\'' => {
                                            chars.next();
                                            out.push_str(&utf16_to_utf8(after));
                                        }
                                        '$' => {
                                            chars.next();
                                            out.push('$');
                                        }
                                        '0'..='9' => {
                                            let mut num_str = String::new();
                                            num_str.push(next);
                                            chars.next();
                                            while let Some(&digit @ '0'..='9') = chars.peek() {
                                                num_str.push(digit);
                                                chars.next();
                                            }
                                            if let Ok(n) = num_str.parse::<usize>() {
                                                if n > 0 && n <= captures.len() {
                                                    if let Some(ref cap) = captures[n - 1] {
                                                        out.push_str(&utf16_to_utf8(cap));
                                                    }
                                                } else {
                                                    out.push('$');
                                                    out.push_str(&num_str);
                                                }
                                            }
                                        }
                                        _ => {
                                            out.push('$');
                                        }
                                    }
                                } else {
                                    out.push('$');
                                }
                            } else {
                                out.push(ch);
                            }
                        }
                        utf8_to_utf16(&out)
                    }

                    let mut offset = 0usize;
                    while let Some(m) = re.find_from_utf16(s, offset).next() {
                        let start = m.range.start;
                        let end = m.range.end;

                        let before = &s[..start];
                        let after = &s[end..];
                        let matched = &s[start..end];

                        let mut captures = Vec::new();
                        for cap in m.captures.iter() {
                            if let Some(range) = cap {
                                captures.push(Some(s[range.start..range.end].to_vec()));
                            } else {
                                captures.push(None);
                            }
                        }

                        out.extend_from_slice(&s[last_pos..start]);
                        out.extend_from_slice(&expand_replacement(&repl, matched, &captures, before, after));
                        last_pos = end;

                        if start == end {
                            offset = end + 1;
                        } else {
                            offset = end;
                        }
                        if offset > s.len() {
                            break;
                        }
                    }

                    out.extend_from_slice(&s[last_pos..]);
                    Ok(Value::String(out))
                } else {
                    Err(raise_eval_error!(
                        "replaceAll only supports string as replacement argument for RegExp search"
                    ))
                }
            } else {
                Err(raise_eval_error!("replaceAll: search argument must be a string or RegExp"))
            }
        } else if let (Value::String(search), Value::String(replace)) = (search_val, replace_val) {
            // String replaceAll
            let mut out = Vec::new();
            let mut last_pos = 0;
            let mut start = 0;
            while let Some(pos) = utf16_find(&s[start..], &search) {
                let abs_pos = start + pos;
                out.extend_from_slice(&s[last_pos..abs_pos]);
                out.extend_from_slice(&replace);
                last_pos = abs_pos + search.len();
                start = last_pos;
            }
            out.extend_from_slice(&s[last_pos..]);
            Ok(Value::String(out))
        } else {
            Err(raise_eval_error!("replaceAll: both arguments must be strings"))
        }
    } else {
        Err(raise_eval_error!(format!(
            "replaceAll method expects 2 arguments, got {}",
            args.len()
        )))
    }
}
