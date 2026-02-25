use crate::core::{
    EvalError, InternalSlot, JSObjectDataPtr, MutationContext, Value, env_set, get_own_property, new_js_object_data, object_get_key_value,
    object_set_key_value, slot_get, slot_set,
};
use crate::error::JSError;
use crate::js_array::{create_array, set_array_length};
use crate::unicode::{utf8_to_utf16, utf16_to_utf8};
use regress::Regex;

pub fn initialize_regexp<'gc>(mc: &MutationContext<'gc>, env: &JSObjectDataPtr<'gc>) -> Result<(), JSError> {
    let regexp_ctor = new_js_object_data(mc);
    slot_set(mc, &regexp_ctor, InternalSlot::IsConstructor, &Value::Boolean(true));
    slot_set(mc, &regexp_ctor, InternalSlot::NativeCtor, &Value::String(utf8_to_utf16("RegExp")));

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
    regexp_ctor.borrow_mut(mc).set_non_enumerable("prototype");
    regexp_ctor.borrow_mut(mc).set_non_writable("prototype");
    regexp_ctor.borrow_mut(mc).set_non_configurable("prototype");
    object_set_key_value(mc, &regexp_proto, "constructor", &Value::Object(regexp_ctor))?;

    // Register instance methods
    let methods = vec!["exec", "test", "toString"];

    for method in methods {
        let val = Value::Function(format!("RegExp.prototype.{method}"));
        object_set_key_value(mc, &regexp_proto, method, &val)?;
        regexp_proto.borrow_mut(mc).set_non_enumerable(method);
    }

    if let Some(sym_ctor_val) = object_get_key_value(env, "Symbol")
        && let Value::Object(sym_ctor) = &*sym_ctor_val.borrow()
    {
        if let Some(match_sym_val) = object_get_key_value(sym_ctor, "match")
            && let Value::Symbol(match_sym) = &*match_sym_val.borrow()
        {
            let match_fn = Value::Function("RegExp.prototype.match".to_string());
            object_set_key_value(mc, &regexp_proto, *match_sym, &match_fn)?;
            regexp_proto.borrow_mut(mc).set_non_enumerable(*match_sym);
        }

        // Register Symbol.replace, Symbol.search, Symbol.split on RegExp.prototype
        for sym_name in ["replace", "search", "split"] {
            if let Some(sym_val) = object_get_key_value(sym_ctor, sym_name)
                && let Value::Symbol(sym) = &*sym_val.borrow()
            {
                let fn_val = Value::Function(format!("RegExp.prototype.{sym_name}"));
                object_set_key_value(mc, &regexp_proto, *sym, &fn_val)?;
                regexp_proto.borrow_mut(mc).set_non_enumerable(*sym);
            }
        }

        if let Some(species_sym_val) = object_get_key_value(sym_ctor, "species")
            && let Value::Symbol(species_sym) = &*species_sym_val.borrow()
        {
            let species_getter = Value::Function("RegExp.species".to_string());
            let species_accessor = Value::Property {
                value: None,
                getter: Some(Box::new(species_getter)),
                setter: None,
            };
            object_set_key_value(mc, &regexp_ctor, *species_sym, &species_accessor)?;
            regexp_ctor.borrow_mut(mc).set_non_enumerable(*species_sym);
        }
    }

    // Register accessor properties on RegExp.prototype per spec
    let accessor_props = vec![
        "source",
        "global",
        "ignoreCase",
        "multiline",
        "dotAll",
        "unicode",
        "sticky",
        "hasIndices",
        "unicodeSets",
        "flags",
    ];
    for prop_name in accessor_props {
        let getter = Value::Function(format!("RegExp.prototype.get {prop_name}"));
        let accessor = Value::Property {
            value: None,
            getter: Some(Box::new(getter)),
            setter: None,
        };
        object_set_key_value(mc, &regexp_proto, prop_name, &accessor)?;
        regexp_proto.borrow_mut(mc).set_non_enumerable(prop_name);
    }

    regexp_proto.borrow_mut(mc).set_non_enumerable("constructor");

    env_set(mc, env, "RegExp", &Value::Object(regexp_ctor))?;

    // Ensure RegExp constructor [[Prototype]] = Function.prototype
    if let Err(e) = crate::core::set_internal_prototype_from_constructor(mc, &regexp_ctor, env, "Function") {
        log::warn!("Failed to set RegExp constructor's internal prototype from Function: {e:?}");
    }

    Ok(())
}

pub fn internal_get_regex_pattern(obj: &JSObjectDataPtr) -> Result<Vec<u16>, JSError> {
    match slot_get(obj, &InternalSlot::Regex) {
        Some(val) => match &*val.borrow() {
            Value::String(s) => Ok(s.clone()),
            _ => Err(raise_type_error!("Invalid regex pattern")),
        },
        None => Err(raise_type_error!("Invalid regex object")),
    }
}

/// Handle RegExp.prototype getter accessors (source, global, ignoreCase, etc.)
/// These read internal slots from the RegExp instance (`this`).
pub(crate) fn handle_regexp_getter<'gc>(obj: &JSObjectDataPtr<'gc>, prop: &str) -> Result<Option<Value<'gc>>, EvalError<'gc>> {
    // Check if obj is actually a RegExp (has __regex internal slot)
    let is_regexp = slot_get(obj, &InternalSlot::Regex).is_some();
    if !is_regexp {
        // Per spec, accessing these on non-RegExp returns undefined for some,
        // throws TypeError for others. For RegExp.prototype itself, "flags" returns ""
        // and "source" returns "(?:)".
        return Ok(Some(Value::Undefined));
    }
    match prop {
        "source" => {
            if let Some(val) = slot_get(obj, &InternalSlot::Regex) {
                Ok(Some(val.borrow().clone()))
            } else {
                Ok(Some(Value::String(utf8_to_utf16("(?:)"))))
            }
        }
        "global" => Ok(Some(get_bool_slot(obj, &InternalSlot::RegexGlobal))),
        "ignoreCase" => Ok(Some(get_bool_slot(obj, &InternalSlot::RegexIgnoreCase))),
        "multiline" => Ok(Some(get_bool_slot(obj, &InternalSlot::RegexMultiline))),
        "dotAll" => Ok(Some(get_bool_slot(obj, &InternalSlot::RegexDotAll))),
        "unicode" => Ok(Some(get_bool_slot(obj, &InternalSlot::RegexUnicode))),
        "sticky" => Ok(Some(get_bool_slot(obj, &InternalSlot::RegexSticky))),
        "hasIndices" => Ok(Some(get_bool_slot(obj, &InternalSlot::RegexHasIndices))),
        "unicodeSets" => Ok(Some(get_bool_slot(obj, &InternalSlot::RegexUnicodeSets))),
        "flags" => {
            if let Some(val) = slot_get(obj, &InternalSlot::Flags) {
                Ok(Some(val.borrow().clone()))
            } else {
                Ok(Some(Value::String(utf8_to_utf16(""))))
            }
        }
        _ => Ok(None),
    }
}

fn get_bool_slot<'gc>(obj: &JSObjectDataPtr<'gc>, slot: &InternalSlot) -> Value<'gc> {
    if let Some(val) = slot_get(obj, slot) {
        val.borrow().clone()
    } else {
        Value::Boolean(false)
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
    let flags = match slot_get(obj, &InternalSlot::Flags) {
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
    env: Option<&JSObjectDataPtr<'gc>>,
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
    if let Some(env) = env
        && let Some(regexp_ctor_val) = object_get_key_value(env, "RegExp")
        && let Value::Object(regexp_ctor_obj) = &*regexp_ctor_val.borrow()
        && let Some(regexp_proto_val) = object_get_key_value(regexp_ctor_obj, "prototype")
        && let Value::Object(regexp_proto_obj) = &*regexp_proto_val.borrow()
    {
        regexp_obj.borrow_mut(mc).prototype = Some(*regexp_proto_obj);
    }
    slot_set(mc, &regexp_obj, InternalSlot::Regex, &Value::String(pattern_u16.clone()));
    slot_set(mc, &regexp_obj, InternalSlot::Flags, &Value::String(utf8_to_utf16(&flags)));
    slot_set(mc, &regexp_obj, InternalSlot::RegexGlobal, &Value::Boolean(global));
    slot_set(mc, &regexp_obj, InternalSlot::RegexIgnoreCase, &Value::Boolean(ignore_case));
    slot_set(mc, &regexp_obj, InternalSlot::RegexMultiline, &Value::Boolean(multiline));
    slot_set(mc, &regexp_obj, InternalSlot::RegexDotAll, &Value::Boolean(dot_matches_new_line));
    slot_set(mc, &regexp_obj, InternalSlot::RegexUnicode, &Value::Boolean(unicode));
    slot_set(mc, &regexp_obj, InternalSlot::RegexSticky, &Value::Boolean(sticky));
    slot_set(mc, &regexp_obj, InternalSlot::SwapGreed, &Value::Boolean(swap_greed));
    slot_set(mc, &regexp_obj, InternalSlot::Crlf, &Value::Boolean(crlf));
    slot_set(mc, &regexp_obj, InternalSlot::RegexHasIndices, &Value::Boolean(has_indices));
    slot_set(mc, &regexp_obj, InternalSlot::RegexUnicodeSets, &Value::Boolean(unicode_sets));

    object_set_key_value(mc, &regexp_obj, "lastIndex", &Value::Number(0.0))?;
    regexp_obj.borrow_mut(mc).set_non_enumerable("lastIndex");
    regexp_obj.borrow_mut(mc).set_non_configurable("lastIndex");

    // Per spec, source/global/flags/etc. are accessor properties on RegExp.prototype,
    // not own data properties on instances. The prototype getters read internal slots
    // (__regex, __global, __flags, etc.) from the instance via `this`.
    // Do NOT set per-instance data properties for these.

    Ok(Value::Object(regexp_obj))
}

pub(crate) fn create_regexp_object_fast_for_eval<'gc>(
    mc: &MutationContext<'gc>,
    env: &JSObjectDataPtr<'gc>,
    pattern_u16: Vec<u16>,
    flags: String,
) -> Result<Value<'gc>, EvalError<'gc>> {
    create_regexp_object_from_parts(mc, Some(env), pattern_u16, flags, false)
}

/// Handle RegExp constructor calls
pub(crate) fn handle_regexp_constructor<'gc>(mc: &MutationContext<'gc>, args: &[Value<'gc>]) -> Result<Value<'gc>, EvalError<'gc>> {
    handle_regexp_constructor_with_env(mc, None, args)
}

pub(crate) fn handle_regexp_constructor_with_env<'gc>(
    mc: &MutationContext<'gc>,
    env: Option<&JSObjectDataPtr<'gc>>,
    args: &[Value<'gc>],
) -> Result<Value<'gc>, EvalError<'gc>> {
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

    create_regexp_object_from_parts(mc, env, pattern_u16, flags, true)
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
            let flags = match slot_get(object, &InternalSlot::Flags) {
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
            let flags = match slot_get(object, &InternalSlot::Flags) {
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

            let flags = match slot_get(object, &InternalSlot::Flags) {
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
