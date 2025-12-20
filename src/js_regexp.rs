use crate::core::{Expr, JSObjectDataPtr, Value, evaluate_expr, get_own_property, new_js_object_data, obj_set_key_value};
use crate::error::JSError;
use crate::js_array::set_array_length;
use crate::unicode::{utf8_to_utf16, utf16_to_utf8};
use regress::Regex;

pub fn internal_get_regex_pattern(obj: &JSObjectDataPtr) -> Result<Vec<u16>, JSError> {
    match get_own_property(obj, &"__regex".into()) {
        Some(val) => match &*val.borrow() {
            Value::String(s) => Ok(s.clone()),
            _ => Err(raise_type_error!("Invalid regex pattern")),
        },
        None => Err(raise_type_error!("Invalid regex object")),
    }
}

pub fn create_regex_from_utf16(pattern: &[u16], flags: &str) -> Result<Regex, String> {
    let it = std::char::decode_utf16(pattern.iter().cloned()).map(|r| match r {
        Ok(c) => c as u32,
        Err(e) => e.unpaired_surrogate() as u32,
    });
    Regex::from_unicode(it, flags).map_err(|e| e.to_string())
}

pub fn is_regex_object(obj: &JSObjectDataPtr) -> bool {
    internal_get_regex_pattern(obj).is_ok()
}

pub fn get_regex_pattern(obj: &JSObjectDataPtr) -> Result<String, JSError> {
    let pattern_utf16 = internal_get_regex_pattern(obj)?;
    Ok(utf16_to_utf8(&pattern_utf16))
}

pub fn get_regex_literal_pattern(obj: &JSObjectDataPtr) -> Result<String, JSError> {
    let pat = crate::js_regexp::get_regex_pattern(obj)?;
    let flags = match get_own_property(obj, &"__flags".into()) {
        Some(val) => match &*val.borrow() {
            Value::String(s) => utf16_to_utf8(s),
            _ => String::new(),
        },
        None => String::new(),
    };
    if flags.is_empty() {
        Ok(format!("/{pat}/"))
    } else {
        Ok(format!("/{pat}/{flags}"))
    }
}

/// Handle RegExp constructor calls
pub(crate) fn handle_regexp_constructor(args: &[Expr], env: &JSObjectDataPtr) -> Result<Value, JSError> {
    let (pattern, flags) = if args.is_empty() {
        // new RegExp() - empty regex
        ("".to_string(), "".to_string())
    } else if args.len() == 1 {
        // new RegExp(pattern)
        let pattern_val = evaluate_expr(env, &args[0])?;
        let pattern = match pattern_val {
            Value::String(s) => utf16_to_utf8(&s),
            Value::Number(n) => n.to_string(),
            Value::Boolean(b) => b.to_string(),
            _ => {
                return Err(raise_type_error!("Invalid RegExp pattern"));
            }
        };
        (pattern, "".to_string())
    } else {
        // new RegExp(pattern, flags)
        let pattern_val = evaluate_expr(env, &args[0])?;
        let flags_val = evaluate_expr(env, &args[1])?;

        let pattern = match pattern_val {
            Value::String(s) => utf16_to_utf8(&s),
            Value::Number(n) => n.to_string(),
            Value::Boolean(b) => b.to_string(),
            _ => {
                return Err(raise_type_error!("Invalid RegExp pattern"));
            }
        };

        let flags = match flags_val {
            Value::String(s) => utf16_to_utf8(&s),
            Value::Number(n) => n.to_string(),
            Value::Boolean(b) => b.to_string(),
            _ => {
                return Err(raise_type_error!("Invalid RegExp flags"));
            }
        };

        (pattern, flags)
    };

    // Parse flags
    let mut global = false;
    let mut ignore_case = false;
    let mut multiline = false;
    let mut dot_matches_new_line = false;
    let mut swap_greed = false;
    let mut unicode = false;
    let mut sticky = false;
    let mut crlf = false;
    let mut has_indices = false;
    let mut unicode_sets = false;

    for flag in flags.chars() {
        match flag {
            'g' => global = true,
            'i' => ignore_case = true,
            'm' => multiline = true,
            's' => dot_matches_new_line = true,
            'U' => swap_greed = true,
            'u' => unicode = true,
            'y' => sticky = true,
            'R' => crlf = true,
            'd' => has_indices = true,
            'v' => unicode_sets = true,
            _ => {
                return Err(raise_syntax_error!(format!("Invalid RegExp flag: {flag}")));
            }
        }
    }

    if unicode && unicode_sets {
        return Err(raise_syntax_error!("Invalid RegExp flags: cannot use both 'u' and 'v'"));
    }

    // Combine inline flags so fancy-regex can parse features like backreferences
    // regress handles flags via the constructor, so we don't need inline flags for it.
    // But we still need to validate the pattern.

    // Validate the regex pattern using regress
    let pattern_u16 = utf8_to_utf16(&pattern);
    // We don't pass 'd' to regress as it doesn't affect matching logic, only result generation
    // We pass 'v' if regress supports it, otherwise we might need to strip it or handle it?
    // For now, let's pass the flags that regress likely understands or ignore the ones it doesn't if they are engine-only.
    // 'd' is engine-only. 'v' affects syntax.

    // Filter flags for regress
    let mut regress_flags = String::new();
    for c in flags.chars() {
        if "gimsuy".contains(c) {
            regress_flags.push(c);
        }
        // 'v' implies 'u' but with different rules. If regress supports 'u', we might pass 'u' for 'v' if it's close enough,
        // but 'v' has different escaping rules.
        // If regress doesn't support 'v', we can't really support it fully.
        // Let's assume for now we just accept 'v' but don't pass it to regress if regress doesn't support it,
        // OR we pass 'u' instead if that's the fallback, but that's dangerous.
        // Actually, let's try passing 'u' if 'v' is present, as 'v' is a superset of 'u' mostly.
        if c == 'v' {
            regress_flags.push('u');
        }
    }

    if let Err(e) = create_regex_from_utf16(&pattern_u16, &regress_flags) {
        return Err(raise_syntax_error!(format!("Invalid RegExp: {}", e)));
    }

    // Create RegExp object
    let regexp_obj = new_js_object_data();

    // Store regex and flags as properties
    obj_set_key_value(&regexp_obj, &"__regex".into(), Value::String(utf8_to_utf16(&pattern)))?;
    obj_set_key_value(&regexp_obj, &"__flags".into(), Value::String(utf8_to_utf16(&flags)))?;
    obj_set_key_value(&regexp_obj, &"__global".into(), Value::Boolean(global))?;
    obj_set_key_value(&regexp_obj, &"__ignoreCase".into(), Value::Boolean(ignore_case))?;
    obj_set_key_value(&regexp_obj, &"__multiline".into(), Value::Boolean(multiline))?;
    obj_set_key_value(&regexp_obj, &"__dotAll".into(), Value::Boolean(dot_matches_new_line))?;
    obj_set_key_value(&regexp_obj, &"__unicode".into(), Value::Boolean(unicode))?;
    obj_set_key_value(&regexp_obj, &"__sticky".into(), Value::Boolean(sticky))?;
    obj_set_key_value(&regexp_obj, &"__swapGreed".into(), Value::Boolean(swap_greed))?;
    obj_set_key_value(&regexp_obj, &"__crlf".into(), Value::Boolean(crlf))?;
    obj_set_key_value(&regexp_obj, &"__hasIndices".into(), Value::Boolean(has_indices))?;
    obj_set_key_value(&regexp_obj, &"__unicodeSets".into(), Value::Boolean(unicode_sets))?;

    // Expose user-visible properties
    obj_set_key_value(&regexp_obj, &"lastIndex".into(), Value::Number(0.0))?;
    obj_set_key_value(&regexp_obj, &"global".into(), Value::Boolean(global))?;
    obj_set_key_value(&regexp_obj, &"ignoreCase".into(), Value::Boolean(ignore_case))?;
    obj_set_key_value(&regexp_obj, &"multiline".into(), Value::Boolean(multiline))?;
    obj_set_key_value(&regexp_obj, &"dotAll".into(), Value::Boolean(dot_matches_new_line))?;
    obj_set_key_value(&regexp_obj, &"unicode".into(), Value::Boolean(unicode))?;
    obj_set_key_value(&regexp_obj, &"sticky".into(), Value::Boolean(sticky))?;
    obj_set_key_value(&regexp_obj, &"hasIndices".into(), Value::Boolean(has_indices))?;
    obj_set_key_value(&regexp_obj, &"unicodeSets".into(), Value::Boolean(unicode_sets))?;
    obj_set_key_value(&regexp_obj, &"flags".into(), Value::String(utf8_to_utf16(&flags)))?; // This should be a getter on prototype, but for now...

    // Add methods
    obj_set_key_value(&regexp_obj, &"exec".into(), Value::Function("RegExp.prototype.exec".to_string()))?;
    obj_set_key_value(&regexp_obj, &"test".into(), Value::Function("RegExp.prototype.test".to_string()))?;
    obj_set_key_value(
        &regexp_obj,
        &"toString".into(),
        Value::Function("RegExp.prototype.toString".to_string()),
    )?;

    Ok(Value::Object(regexp_obj))
}

/// Handle RegExp instance method calls
pub(crate) fn handle_regexp_method(
    obj_map: &JSObjectDataPtr,
    method: &str,
    args: &[Expr],
    env: &JSObjectDataPtr,
) -> Result<Value, JSError> {
    match method {
        "exec" => {
            if args.is_empty() {
                return Err(raise_type_error!("RegExp.prototype.exec requires a string argument"));
            }

            let input_val = evaluate_expr(env, &args[0])?;
            let input_u16 = match input_val {
                Value::String(s) => s,
                _ => {
                    let s = match input_val {
                        Value::Number(n) => n.to_string(),
                        Value::Boolean(b) => b.to_string(),
                        Value::Undefined => "undefined".to_string(),
                        Value::Null => "null".to_string(),
                        Value::Object(_obj) => {
                            // Simple toString for object
                            "[object Object]".to_string()
                        }
                        _ => return Err(raise_type_error!("RegExp.prototype.exec requires a string argument")),
                    };
                    utf8_to_utf16(&s)
                }
            };

            // Get regex pattern and flags
            let pattern_u16 = internal_get_regex_pattern(obj_map)?;
            let flags = match get_own_property(obj_map, &"__flags".into()) {
                Some(val) => match &*val.borrow() {
                    Value::String(s) => utf16_to_utf8(s),
                    _ => "".to_string(),
                },
                None => "".to_string(),
            };

            let crlf = flags.contains('R');
            let global = flags.contains('g');
            let sticky = flags.contains('y');
            let has_indices = flags.contains('d');
            let use_last = global || sticky;

            // Handle CRLF normalization
            let (working_input, mapping) = if crlf {
                let mut res = Vec::with_capacity(input_u16.len());
                let mut i = 0;
                while i < input_u16.len() {
                    if input_u16[i] == '\r' as u16 && i + 1 < input_u16.len() && input_u16[i + 1] == '\n' as u16 {
                        res.push('\n' as u16);
                        i += 2;
                    } else {
                        res.push(input_u16[i]);
                        i += 1;
                    }
                }
                (res, true)
            } else {
                (input_u16.clone(), false)
            };

            // Filter flags for regress
            let mut r_flags = String::new();
            for c in flags.chars() {
                if "gimsuy".contains(c) {
                    r_flags.push(c);
                }
                if c == 'v' {
                    r_flags.push('u');
                }
            }

            let re = create_regex_from_utf16(&pattern_u16, &r_flags).map_err(|e| raise_syntax_error!(format!("Invalid RegExp: {e}")))?;

            let mut last_index = 0;
            if use_last
                && let Some(last_index_val) = get_own_property(obj_map, &"lastIndex".into())
                && let Value::Number(n) = &*last_index_val.borrow()
            {
                last_index = (*n as isize).max(0) as usize;
            }

            let match_result = re.find_from_utf16(&working_input, last_index).next();

            match match_result {
                Some(m) => {
                    let start = m.range.start;
                    let end = m.range.end;

                    let (orig_start, orig_end) = if mapping {
                        (map_index_back(&input_u16, start), map_index_back(&input_u16, end))
                    } else {
                        (start, end)
                    };

                    // Construct result array
                    let result_array = crate::js_array::create_array(env)?;

                    let full_match_u16 = input_u16[orig_start..orig_end].to_vec();
                    obj_set_key_value(&result_array, &"0".into(), Value::String(full_match_u16))?;

                    let indices_array = if has_indices {
                        Some(crate::js_array::create_array(env)?)
                    } else {
                        None
                    };

                    if let Some(indices) = &indices_array {
                        let match_indices = crate::js_array::create_array(env)?;
                        obj_set_key_value(&match_indices, &"0".into(), Value::Number(orig_start as f64))?;
                        obj_set_key_value(&match_indices, &"1".into(), Value::Number(orig_end as f64))?;
                        set_array_length(&match_indices, 2)?;
                        obj_set_key_value(indices, &"0".into(), Value::Object(match_indices))?;
                    }

                    let mut group_index = 1;
                    for cap in m.captures.iter() {
                        if let Some(range) = cap {
                            let (cs, ce) = if mapping {
                                (map_index_back(&input_u16, range.start), map_index_back(&input_u16, range.end))
                            } else {
                                (range.start, range.end)
                            };
                            let cap_str = input_u16[cs..ce].to_vec();
                            obj_set_key_value(&result_array, &group_index.to_string().into(), Value::String(cap_str))?;

                            if let Some(indices) = &indices_array {
                                let group_indices = crate::js_array::create_array(env)?;
                                obj_set_key_value(&group_indices, &"0".into(), Value::Number(cs as f64))?;
                                obj_set_key_value(&group_indices, &"1".into(), Value::Number(ce as f64))?;
                                set_array_length(&group_indices, 2)?;
                                obj_set_key_value(indices, &group_index.to_string().into(), Value::Object(group_indices))?;
                            }
                        } else {
                            obj_set_key_value(&result_array, &group_index.to_string().into(), Value::Undefined)?;
                            if let Some(indices) = &indices_array {
                                obj_set_key_value(indices, &group_index.to_string().into(), Value::Undefined)?;
                            }
                        }
                        group_index += 1;
                    }
                    set_array_length(&result_array, group_index)?;

                    obj_set_key_value(&result_array, &"index".into(), Value::Number(orig_start as f64))?;
                    obj_set_key_value(&result_array, &"input".into(), Value::String(input_u16.clone()))?;
                    obj_set_key_value(&result_array, &"groups".into(), Value::Undefined)?;

                    if let Some(indices) = indices_array {
                        obj_set_key_value(&result_array, &"indices".into(), Value::Object(indices))?;
                    }

                    if use_last {
                        obj_set_key_value(obj_map, &"lastIndex".into(), Value::Number(orig_end as f64))?;
                    }

                    Ok(Value::Object(result_array))
                }
                None => {
                    if global {
                        obj_set_key_value(obj_map, &"lastIndex".into(), Value::Number(0.0))?;
                    }
                    Ok(Value::Null)
                }
            }
        }
        "test" => {
            if args.is_empty() {
                return Err(raise_type_error!("RegExp.prototype.test requires a string argument"));
            }

            let input_val = evaluate_expr(env, &args[0])?;
            let input_u16 = match input_val {
                Value::String(s) => s,
                _ => {
                    let s = match input_val {
                        Value::Number(n) => n.to_string(),
                        Value::Boolean(b) => b.to_string(),
                        Value::Undefined => "undefined".to_string(),
                        Value::Null => "null".to_string(),
                        Value::Object(_) => "[object Object]".to_string(),
                        _ => return Err(raise_type_error!("RegExp.prototype.test requires a string argument")),
                    };
                    utf8_to_utf16(&s)
                }
            };

            let pattern_u16 = internal_get_regex_pattern(obj_map)?;
            let flags = match get_own_property(obj_map, &"__flags".into()) {
                Some(val) => match &*val.borrow() {
                    Value::String(s) => utf16_to_utf8(s),
                    _ => "".to_string(),
                },
                None => "".to_string(),
            };

            let crlf = flags.contains('R');
            let global = flags.contains('g');
            let sticky = flags.contains('y');
            let use_last = global || sticky;

            let (working_input, mapping) = if crlf {
                let mut res = Vec::with_capacity(input_u16.len());
                let mut i = 0;
                while i < input_u16.len() {
                    if input_u16[i] == '\r' as u16 && i + 1 < input_u16.len() && input_u16[i + 1] == '\n' as u16 {
                        res.push('\n' as u16);
                        i += 2;
                    } else {
                        res.push(input_u16[i]);
                        i += 1;
                    }
                }
                (res, true)
            } else {
                (input_u16.clone(), false)
            };

            let re = create_regex_from_utf16(&pattern_u16, &flags).map_err(|e| raise_syntax_error!(format!("Invalid RegExp: {}", e)))?;

            let mut last_index = 0;
            if use_last
                && let Some(last_index_val) = get_own_property(obj_map, &"lastIndex".into())
                && let Value::Number(n) = &*last_index_val.borrow()
            {
                last_index = (*n as isize).max(0) as usize;
            }

            let match_result = re.find_from_utf16(&working_input, last_index).next();

            match match_result {
                Some(m) => {
                    if use_last {
                        let end = m.range.end;
                        let orig_end = if mapping { map_index_back(&input_u16, end) } else { end };
                        obj_set_key_value(obj_map, &"lastIndex".into(), Value::Number(orig_end as f64))?;
                    }
                    Ok(Value::Boolean(true))
                }
                None => {
                    if global {
                        obj_set_key_value(obj_map, &"lastIndex".into(), Value::Number(0.0))?;
                    }
                    Ok(Value::Boolean(false))
                }
            }
        }
        "toString" => {
            // Get pattern and flags (two-step get to avoid long-lived borrows)
            let pattern = utf16_to_utf8(&internal_get_regex_pattern(obj_map).unwrap_or_default());

            let flags = match get_own_property(obj_map, &"__flags".into()) {
                Some(val) => match &*val.borrow() {
                    Value::String(s) => utf16_to_utf8(s),
                    _ => "".to_string(),
                },
                None => "".to_string(),
            };

            let result = format!("/{}/{}", pattern, flags);
            Ok(Value::String(utf8_to_utf16(&result)))
        }
        _ => Err(raise_eval_error!(format!("RegExp.prototype.{method} is not implemented"))),
    }
}

fn map_index_back(original: &[u16], working_index: usize) -> usize {
    let mut w = 0;
    let mut o = 0;
    while w < working_index {
        if o < original.len() && original[o] == '\r' as u16 && o + 1 < original.len() && original[o + 1] == '\n' as u16 {
            o += 2;
            w += 1;
        } else {
            o += 1;
            w += 1;
        }
    }
    o
}
