use crate::core::{
    EvalError, JSObjectDataPtr, MutationContext, Value, env_set, get_own_property, new_js_object_data, object_get_key_value,
    object_set_key_value,
};
use crate::error::JSError;
use crate::js_array::{create_array, set_array_length};
use crate::unicode::{utf8_to_utf16, utf16_to_utf8};
use regress::Regex;

pub fn initialize_regexp<'gc>(mc: &MutationContext<'gc>, env: &JSObjectDataPtr<'gc>) -> Result<(), JSError> {
    let regexp_ctor = new_js_object_data(mc);
    object_set_key_value(mc, &regexp_ctor, "__is_constructor", &Value::Boolean(true))?;
    object_set_key_value(mc, &regexp_ctor, "__native_ctor", &Value::String(utf8_to_utf16("RegExp")))?;

    // Get Object.prototype
    let object_proto = if let Some(obj_val) = object_get_key_value(env, "Object")
        && let Value::Object(obj_ctor) = &*obj_val.borrow()
        && let Some(proto_val) = object_get_key_value(obj_ctor, "prototype")
        && let Value::Object(proto) = &*proto_val.borrow()
    {
        Some(*proto)
    } else {
        None
    };

    let regexp_proto = new_js_object_data(mc);
    if let Some(proto) = object_proto {
        regexp_proto.borrow_mut(mc).prototype = Some(proto);
    }

    object_set_key_value(mc, &regexp_ctor, "prototype", &Value::Object(regexp_proto))?;
    object_set_key_value(mc, &regexp_proto, "constructor", &Value::Object(regexp_ctor))?;

    // Register instance methods
    let methods = vec!["exec", "test", "toString"];

    for method in methods {
        let val = Value::Function(format!("RegExp.prototype.{method}"));
        object_set_key_value(mc, &regexp_proto, method, &val)?;
        regexp_proto.borrow_mut(mc).set_non_enumerable(method);
    }
    regexp_proto.borrow_mut(mc).set_non_enumerable("constructor");

    env_set(mc, env, "RegExp", &Value::Object(regexp_ctor))?;
    Ok(())
}

pub fn internal_get_regex_pattern(obj: &JSObjectDataPtr) -> Result<Vec<u16>, JSError> {
    match get_own_property(obj, "__regex") {
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

fn contains_lone_surrogate(units: &[u16]) -> bool {
    let mut i = 0;
    while i < units.len() {
        let u = units[i];
        if (0xD800..=0xDBFF).contains(&u) {
            if i + 1 >= units.len() {
                return true;
            }
            let next = units[i + 1];
            if !(0xDC00..=0xDFFF).contains(&next) {
                return true;
            }
            i += 2;
            continue;
        }
        if (0xDC00..=0xDFFF).contains(&u) {
            return true;
        }
        i += 1;
    }
    false
}

fn validate_named_group_identifiers(pattern_u16: &[u16]) -> Result<(), JSError> {
    let mut i = 0usize;
    while i + 2 < pattern_u16.len() {
        if pattern_u16[i] == b'(' as u16 && pattern_u16[i + 1] == b'?' as u16 && pattern_u16[i + 2] == b'<' as u16 {
            if i + 3 < pattern_u16.len() {
                let next = pattern_u16[i + 3];
                if next == b'=' as u16 || next == b'!' as u16 {
                    i += 1;
                    continue;
                }
            }

            let mut j = i + 3;
            while j < pattern_u16.len() && pattern_u16[j] != b'>' as u16 {
                j += 1;
            }

            if j < pattern_u16.len() {
                if contains_lone_surrogate(&pattern_u16[i + 3..j]) {
                    return Err(raise_syntax_error!("Invalid token at named capture group identifier"));
                }
                i = j;
            }
        }
        i += 1;
    }
    Ok(())
}

pub fn is_regex_object(obj: &JSObjectDataPtr) -> bool {
    internal_get_regex_pattern(obj).is_ok()
}

pub fn get_regex_pattern(obj: &JSObjectDataPtr) -> Result<String, JSError> {
    let pattern_utf16 = internal_get_regex_pattern(obj)?;
    Ok(utf16_to_utf8(&pattern_utf16))
}

pub fn get_regex_literal_pattern(obj: &JSObjectDataPtr) -> Result<String, JSError> {
    let pat = get_regex_pattern(obj)?;
    let flags = match get_own_property(obj, "__flags") {
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

fn create_regexp_object_from_parts<'gc>(
    mc: &MutationContext<'gc>,
    pattern_u16: Vec<u16>,
    flags: String,
    validate_pattern: bool,
) -> Result<Value<'gc>, EvalError<'gc>> {
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
                return Err(raise_syntax_error!(format!("Invalid RegExp flag: {flag}")).into());
            }
        }
    }

    if unicode && unicode_sets {
        return Err(raise_syntax_error!("Invalid RegExp flags: cannot use both 'u' and 'v'").into());
    }

    // Validate named capture identifiers to reject malformed UTF-16 names
    // (e.g. lone surrogates), matching Test262 expectations.
    validate_named_group_identifiers(&pattern_u16).map_err(EvalError::from)?;

    let mut regress_flags = String::new();
    for c in flags.chars() {
        if "gimsuy".contains(c) {
            regress_flags.push(c);
        }
        if c == 'v' {
            regress_flags.push('u');
        }
    }

    if validate_pattern && let Err(e) = create_regex_from_utf16(&pattern_u16, &regress_flags) {
        return Err(raise_syntax_error!(format!("Invalid RegExp: {}", e)).into());
    }

    let regexp_obj = new_js_object_data(mc);
    object_set_key_value(mc, &regexp_obj, "__regex", &Value::String(pattern_u16.clone()))?;
    object_set_key_value(mc, &regexp_obj, "__flags", &Value::String(utf8_to_utf16(&flags)))?;
    object_set_key_value(mc, &regexp_obj, "__global", &Value::Boolean(global))?;
    object_set_key_value(mc, &regexp_obj, "__ignoreCase", &Value::Boolean(ignore_case))?;
    object_set_key_value(mc, &regexp_obj, "__multiline", &Value::Boolean(multiline))?;
    object_set_key_value(mc, &regexp_obj, "__dotAll", &Value::Boolean(dot_matches_new_line))?;
    object_set_key_value(mc, &regexp_obj, "__unicode", &Value::Boolean(unicode))?;
    object_set_key_value(mc, &regexp_obj, "__sticky", &Value::Boolean(sticky))?;
    object_set_key_value(mc, &regexp_obj, "__swapGreed", &Value::Boolean(swap_greed))?;
    object_set_key_value(mc, &regexp_obj, "__crlf", &Value::Boolean(crlf))?;
    object_set_key_value(mc, &regexp_obj, "__hasIndices", &Value::Boolean(has_indices))?;
    object_set_key_value(mc, &regexp_obj, "__unicodeSets", &Value::Boolean(unicode_sets))?;

    object_set_key_value(mc, &regexp_obj, "lastIndex", &Value::Number(0.0))?;
    regexp_obj.borrow_mut(mc).set_non_enumerable("lastIndex");
    regexp_obj.borrow_mut(mc).set_non_configurable("lastIndex");

    object_set_key_value(mc, &regexp_obj, "source", &Value::String(pattern_u16))?;
    regexp_obj.borrow_mut(mc).set_non_enumerable("source");
    regexp_obj.borrow_mut(mc).set_non_configurable("source");
    regexp_obj.borrow_mut(mc).set_non_writable("source");

    object_set_key_value(mc, &regexp_obj, "global", &Value::Boolean(global))?;
    object_set_key_value(mc, &regexp_obj, "ignoreCase", &Value::Boolean(ignore_case))?;
    object_set_key_value(mc, &regexp_obj, "multiline", &Value::Boolean(multiline))?;
    object_set_key_value(mc, &regexp_obj, "dotAll", &Value::Boolean(dot_matches_new_line))?;
    object_set_key_value(mc, &regexp_obj, "unicode", &Value::Boolean(unicode))?;
    object_set_key_value(mc, &regexp_obj, "sticky", &Value::Boolean(sticky))?;
    object_set_key_value(mc, &regexp_obj, "hasIndices", &Value::Boolean(has_indices))?;
    object_set_key_value(mc, &regexp_obj, "unicodeSets", &Value::Boolean(unicode_sets))?;
    object_set_key_value(mc, &regexp_obj, "flags", &Value::String(utf8_to_utf16(&flags)))?;

    object_set_key_value(mc, &regexp_obj, "exec", &Value::Function("RegExp.prototype.exec".to_string()))?;
    object_set_key_value(mc, &regexp_obj, "test", &Value::Function("RegExp.prototype.test".to_string()))?;
    object_set_key_value(
        mc,
        &regexp_obj,
        "toString",
        &Value::Function("RegExp.prototype.toString".to_string()),
    )?;

    Ok(Value::Object(regexp_obj))
}

pub(crate) fn create_regexp_object_fast_for_eval<'gc>(
    mc: &MutationContext<'gc>,
    pattern_u16: Vec<u16>,
    flags: String,
) -> Result<Value<'gc>, EvalError<'gc>> {
    create_regexp_object_from_parts(mc, pattern_u16, flags, false)
}

/// Handle RegExp constructor calls
pub(crate) fn handle_regexp_constructor<'gc>(mc: &MutationContext<'gc>, args: &[Value<'gc>]) -> Result<Value<'gc>, EvalError<'gc>> {
    let (pattern_u16, flags) = if args.is_empty() {
        (Vec::new(), String::new())
    } else if args.len() == 1 {
        let pattern_u16 = match &args[0] {
            Value::String(s) => s.clone(),
            Value::Number(n) => utf8_to_utf16(&n.to_string()),
            Value::Boolean(b) => utf8_to_utf16(&b.to_string()),
            _ => {
                return Err(raise_type_error!("Invalid RegExp pattern").into());
            }
        };
        (pattern_u16, String::new())
    } else {
        let pattern_u16 = match &args[0] {
            Value::String(s) => s.clone(),
            Value::Number(n) => utf8_to_utf16(&n.to_string()),
            Value::Boolean(b) => utf8_to_utf16(&b.to_string()),
            _ => {
                return Err(raise_type_error!("Invalid RegExp pattern").into());
            }
        };

        let flags = match &args[1] {
            Value::String(s) => utf16_to_utf8(s),
            Value::Number(n) => n.to_string(),
            Value::Boolean(b) => b.to_string(),
            _ => {
                return Err(raise_type_error!("Invalid RegExp flags").into());
            }
        };

        (pattern_u16, flags)
    };

    create_regexp_object_from_parts(mc, pattern_u16, flags, true)
}

/// Handle RegExp instance method calls
pub(crate) fn handle_regexp_method<'gc>(
    mc: &MutationContext<'gc>,
    object: &JSObjectDataPtr<'gc>,
    method: &str,
    args: &[Value<'gc>],
    env: &JSObjectDataPtr<'gc>,
) -> Result<Value<'gc>, EvalError<'gc>> {
    match method {
        "exec" => {
            if args.is_empty() {
                return Err(raise_type_error!("RegExp.prototype.exec requires a string argument").into());
            }

            let input_val = args[0].clone();
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
                        _ => return Err(raise_type_error!("RegExp.prototype.exec requires a string argument").into()),
                    };
                    utf8_to_utf16(&s)
                }
            };

            // Get regex pattern and flags
            let pattern_u16 = internal_get_regex_pattern(object)?;
            let flags = match get_own_property(object, "__flags") {
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
                && let Some(last_index_val) = get_own_property(object, "lastIndex")
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
                    let result_array = create_array(mc, env)?;

                    let full_match_u16 = input_u16[orig_start..orig_end].to_vec();
                    object_set_key_value(mc, &result_array, "0", &Value::String(full_match_u16))?;

                    let indices_array = if has_indices { Some(create_array(mc, env)?) } else { None };
                    let mut groups_obj: Option<JSObjectDataPtr<'gc>> = None;
                    let mut indices_groups_obj: Option<JSObjectDataPtr<'gc>> = None;

                    if let Some(indices) = &indices_array {
                        let match_indices = create_array(mc, env)?;
                        object_set_key_value(mc, &match_indices, "0", &Value::Number(orig_start as f64))?;
                        object_set_key_value(mc, &match_indices, "1", &Value::Number(orig_end as f64))?;
                        set_array_length(mc, &match_indices, 2)?;
                        object_set_key_value(mc, indices, "0", &Value::Object(match_indices))?;
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
                            object_set_key_value(mc, &result_array, group_index, &Value::String(cap_str))?;

                            if let Some(indices) = &indices_array {
                                let group_indices = create_array(mc, env)?;
                                object_set_key_value(mc, &group_indices, "0", &Value::Number(cs as f64))?;
                                object_set_key_value(mc, &group_indices, "1", &Value::Number(ce as f64))?;
                                set_array_length(mc, &group_indices, 2)?;
                                object_set_key_value(mc, indices, group_index, &Value::Object(group_indices))?;
                            }
                        } else {
                            object_set_key_value(mc, &result_array, group_index, &Value::Undefined)?;
                            if let Some(indices) = &indices_array {
                                object_set_key_value(mc, indices, group_index, &Value::Undefined)?;
                            }
                        }
                        group_index += 1;
                    }

                    for (name, range_opt) in m.named_groups() {
                        let groups = groups_obj.get_or_insert_with(|| new_js_object_data(mc));
                        match range_opt {
                            Some(range) => {
                                let (cs, ce) = if mapping {
                                    (map_index_back(&input_u16, range.start), map_index_back(&input_u16, range.end))
                                } else {
                                    (range.start, range.end)
                                };
                                let cap_str = input_u16[cs..ce].to_vec();
                                object_set_key_value(mc, groups, name, &Value::String(cap_str))?;

                                if let Some(indices) = &indices_array {
                                    let indices_groups = indices_groups_obj.get_or_insert_with(|| new_js_object_data(mc));
                                    let group_indices = create_array(mc, env)?;
                                    object_set_key_value(mc, &group_indices, "0", &Value::Number(cs as f64))?;
                                    object_set_key_value(mc, &group_indices, "1", &Value::Number(ce as f64))?;
                                    set_array_length(mc, &group_indices, 2)?;
                                    object_set_key_value(mc, indices_groups, name, &Value::Object(group_indices))?;
                                    let _ = indices;
                                }
                            }
                            None => {
                                object_set_key_value(mc, groups, name, &Value::Undefined)?;
                                if let Some(indices) = &indices_array {
                                    let indices_groups = indices_groups_obj.get_or_insert_with(|| new_js_object_data(mc));
                                    object_set_key_value(mc, indices_groups, name, &Value::Undefined)?;
                                    let _ = indices;
                                }
                            }
                        }
                    }
                    set_array_length(mc, &result_array, group_index)?;

                    object_set_key_value(mc, &result_array, "index", &Value::Number(orig_start as f64))?;
                    object_set_key_value(mc, &result_array, "input", &Value::String(input_u16.clone()))?;
                    if let Some(groups) = groups_obj {
                        object_set_key_value(mc, &result_array, "groups", &Value::Object(groups))?;
                    } else {
                        object_set_key_value(mc, &result_array, "groups", &Value::Undefined)?;
                    }

                    if let Some(indices) = indices_array {
                        if let Some(indices_groups) = indices_groups_obj {
                            object_set_key_value(mc, &indices, "groups", &Value::Object(indices_groups))?;
                        } else {
                            object_set_key_value(mc, &indices, "groups", &Value::Undefined)?;
                        }
                        object_set_key_value(mc, &result_array, "indices", &Value::Object(indices))?;
                    }

                    if use_last {
                        object_set_key_value(mc, object, "lastIndex", &Value::Number(orig_end as f64))?;
                    }

                    Ok(Value::Object(result_array))
                }
                None => {
                    if global {
                        object_set_key_value(mc, object, "lastIndex", &Value::Number(0.0))?;
                    }
                    Ok(Value::Null)
                }
            }
        }
        "test" => {
            if args.is_empty() {
                return Err(raise_type_error!("RegExp.prototype.test requires a string argument").into());
            }

            let input_val = args[0].clone();
            let input_u16 = match input_val {
                Value::String(s) => s,
                _ => {
                    let s = match input_val {
                        Value::Number(n) => n.to_string(),
                        Value::Boolean(b) => b.to_string(),
                        Value::Undefined => "undefined".to_string(),
                        Value::Null => "null".to_string(),
                        Value::Object(_) => "[object Object]".to_string(),
                        _ => return Err(raise_type_error!("RegExp.prototype.test requires a string argument").into()),
                    };
                    utf8_to_utf16(&s)
                }
            };

            let pattern_u16 = internal_get_regex_pattern(object)?;
            let flags = match get_own_property(object, "__flags") {
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
                && let Some(last_index_val) = get_own_property(object, "lastIndex")
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
                        object_set_key_value(mc, object, "lastIndex", &Value::Number(orig_end as f64))?;
                    }
                    Ok(Value::Boolean(true))
                }
                None => {
                    if global {
                        object_set_key_value(mc, object, "lastIndex", &Value::Number(0.0))?;
                    }
                    Ok(Value::Boolean(false))
                }
            }
        }
        "toString" => {
            // Get pattern and flags (two-step get to avoid long-lived borrows)
            let pattern = utf16_to_utf8(&internal_get_regex_pattern(object).unwrap_or_default());

            let flags = match get_own_property(object, "__flags") {
                Some(val) => match &*val.borrow() {
                    Value::String(s) => utf16_to_utf8(s),
                    _ => "".to_string(),
                },
                None => "".to_string(),
            };

            let result = format!("/{}/{}", pattern, flags);
            Ok(Value::String(utf8_to_utf16(&result)))
        }
        _ => Err(raise_eval_error!(format!("RegExp.prototype.{method} is not implemented")).into()),
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
