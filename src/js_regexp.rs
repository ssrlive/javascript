use crate::core::{
    EvalError, InternalSlot, JSObjectDataPtr, MutationContext, Value, env_set, new_js_object_data, object_get_key_value,
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

    // RegExp.length = 2 (per spec §22.2.4: writable:false, enumerable:false, configurable:true)
    object_set_key_value(mc, &regexp_ctor, "length", &Value::Number(2.0))?;
    regexp_ctor.borrow_mut(mc).set_non_enumerable("length");
    regexp_ctor.borrow_mut(mc).set_non_writable("length");

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
    // Mark as RegExp.prototype for getter spec checks
    slot_set(mc, &regexp_proto, InternalSlot::IsRegExpPrototype, &Value::Boolean(true));

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
            let species_getter = Value::Function("RegExp[Symbol.species]".to_string());
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
/// For `flags`, the spec requires reading observable properties from any object.
/// `mc` and `env` are needed for `flags` getter to invoke accessors.
pub(crate) fn handle_regexp_getter_with_this<'gc>(
    mc: &MutationContext<'gc>,
    env: &JSObjectDataPtr<'gc>,
    this_val: &Value<'gc>,
    prop: &str,
) -> Result<Option<Value<'gc>>, EvalError<'gc>> {
    // Step 1: If Type(this) is not Object, throw TypeError
    let obj = match this_val {
        Value::Object(o) => o,
        _ => {
            return Err(raise_type_error!(format!("RegExp.prototype.{prop} getter called on incompatible receiver")).into());
        }
    };

    // Special case: `flags` getter should work on any object per spec §22.2.5.3
    // MUST always read properties observably (never use internal flags slot)
    if prop == "flags" {
        // For RegExp.prototype with no actual regex, return ""
        if slot_get(obj, &InternalSlot::IsRegExpPrototype).is_some() && slot_get(obj, &InternalSlot::Regex).is_none() {
            return Ok(Some(Value::String(Vec::new())));
        }
        // Build flags from observable property reads (spec §22.2.5.3)
        let mut result = String::new();
        let flag_props: &[(&str, char)] = &[
            ("hasIndices", 'd'),
            ("global", 'g'),
            ("ignoreCase", 'i'),
            ("multiline", 'm'),
            ("dotAll", 's'),
            ("unicode", 'u'),
            ("unicodeSets", 'v'),
            ("sticky", 'y'),
        ];
        for (prop_name, flag_char) in flag_props {
            let val = crate::core::get_property_with_accessors(mc, env, obj, *prop_name)?;
            if val.to_truthy() {
                result.push(*flag_char);
            }
        }
        return Ok(Some(Value::String(utf8_to_utf16(&result))));
    }

    // Check if obj is actually a RegExp (has __regex internal slot)
    let is_regexp = slot_get(obj, &InternalSlot::Regex).is_some();
    if !is_regexp {
        // Step 2: If this does not have [[OriginalFlags]], check if it's %RegExp.prototype%
        if slot_get(obj, &InternalSlot::IsRegExpPrototype).is_some() {
            // RegExp.prototype: source → "(?:)", flag booleans → undefined
            return match prop {
                "source" => Ok(Some(Value::String(utf8_to_utf16("(?:)")))),
                _ => Ok(Some(Value::Undefined)), // global, ignoreCase, etc.
            };
        }
        // Otherwise, throw TypeError
        return Err(raise_type_error!(format!("RegExp.prototype.{prop} getter requires a RegExp object")).into());
    }
    match prop {
        "source" => {
            if let Some(val) = slot_get(obj, &InternalSlot::Regex) {
                if let Value::String(s) = &*val.borrow() {
                    if s.is_empty() {
                        return Ok(Some(Value::String(utf8_to_utf16("(?:)"))));
                    }
                    // EscapeRegExpPattern: escape /, line terminators
                    let escaped = escape_regexp_pattern(s);
                    return Ok(Some(Value::String(escaped)));
                }
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

/// EscapeRegExpPattern — escape forward slashes and line terminators so the
/// source property re-renders as a valid regex literal.
fn escape_regexp_pattern(s: &[u16]) -> Vec<u16> {
    let mut result = Vec::with_capacity(s.len());
    let mut i = 0;
    while i < s.len() {
        let ch = s[i];
        match ch {
            // Forward slash: escape only if not already preceded by backslash
            0x002F => {
                // '/'
                // Check if previous char is a backslash (already escaped)
                let already_escaped = !result.is_empty() && *result.last().unwrap() == 0x005C;
                if !already_escaped {
                    result.push(0x005C); // '\'
                }
                result.push(ch);
            }
            // Line terminators: escape
            0x000A => {
                result.push(0x005C);
                result.push(0x006E);
            } // \n
            0x000D => {
                result.push(0x005C);
                result.push(0x0072);
            } // \r
            0x2028 => {
                result.push(0x005C);
                result.push(0x0075); // \u2028
                result.push(0x0032);
                result.push(0x0030);
                result.push(0x0032);
                result.push(0x0038);
            }
            0x2029 => {
                result.push(0x005C);
                result.push(0x0075); // \u2029
                result.push(0x0032);
                result.push(0x0030);
                result.push(0x0032);
                result.push(0x0039);
            }
            _ => result.push(ch),
        }
        i += 1;
    }
    result
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

    // Check for duplicate flags
    let mut seen_flags = std::collections::HashSet::new();
    for flag in flags.chars() {
        if !seen_flags.insert(flag) {
            return Err(raise_syntax_error!(format!("Duplicate RegExp flag: {flag}")).into());
        }
    }

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

/// §7.2.8 IsRegExp(argument)
/// Returns true if the argument has a truthy @@match property, or has [[RegExpMatcher]] internal slot.
fn is_regexp<'gc>(val: &Value<'gc>, env: Option<&JSObjectDataPtr<'gc>>) -> bool {
    if let Value::Object(obj) = val {
        // Step 1-2: Check @@match property
        if let Some(env) = env
            && let Some(sym_ctor_val) = object_get_key_value(env, "Symbol")
            && let Value::Object(sym_ctor) = &*sym_ctor_val.borrow()
            && let Some(match_sym_val) = object_get_key_value(sym_ctor, "match")
            && let Value::Symbol(match_sym) = &*match_sym_val.borrow()
            && let Some(matcher_val) = object_get_key_value(obj, *match_sym)
        {
            let matcher = matcher_val.borrow().clone();
            if !matches!(matcher, Value::Undefined) {
                return matcher.to_truthy();
            }
        }
        // Step 3: Check [[RegExpMatcher]] internal slot
        return slot_get(obj, &InternalSlot::Regex).is_some();
    }
    false
}

/// Extract pattern and flags from a value that might be a RegExp object.
/// Returns Some((pattern_u16, flags_str)) if the value is a RegExp, None otherwise.
fn extract_regexp_parts<'gc>(val: &Value<'gc>) -> Option<(Vec<u16>, String)> {
    if let Value::Object(obj) = val
        && let Some(pat_rc) = slot_get(obj, &InternalSlot::Regex)
    {
        let pattern = match &*pat_rc.borrow() {
            Value::String(s) => s.clone(),
            _ => Vec::new(),
        };
        let flags = match slot_get(obj, &InternalSlot::Flags) {
            Some(f_rc) => match &*f_rc.borrow() {
                Value::String(s) => utf16_to_utf8(s),
                _ => String::new(),
            },
            None => String::new(),
        };
        return Some((pattern, flags));
    }
    None
}

/// Coerce a value to a pattern string for the RegExp constructor.
fn value_to_pattern_u16<'gc>(val: &Value<'gc>) -> Vec<u16> {
    match val {
        Value::String(s) => s.clone(),
        Value::Number(n) => utf8_to_utf16(&n.to_string()),
        Value::Boolean(b) => utf8_to_utf16(&b.to_string()),
        Value::Null => utf8_to_utf16("null"),
        Value::Undefined => Vec::new(),
        _ => utf8_to_utf16(&crate::core::value_to_string(val)),
    }
}

pub(crate) fn handle_regexp_constructor_with_env<'gc>(
    mc: &MutationContext<'gc>,
    env: Option<&JSObjectDataPtr<'gc>>,
    args: &[Value<'gc>],
) -> Result<Value<'gc>, EvalError<'gc>> {
    let arg0 = args.first().cloned().unwrap_or(Value::Undefined);
    let arg1 = args.get(1).cloned();

    // Step 4: If pattern has [[RegExpMatcher]] internal slot, use internal source/flags
    if let Some((re_pattern, re_flags)) = extract_regexp_parts(&arg0) {
        let flags = if let Some(f) = &arg1 {
            match f {
                Value::Undefined => re_flags,
                Value::String(s) => utf16_to_utf8(s),
                Value::Object(_) => {
                    if let Some(env) = env {
                        let prim = crate::core::to_primitive(mc, f, "string", env)?;
                        crate::core::value_to_string(&prim)
                    } else {
                        crate::core::value_to_string(f)
                    }
                }
                _ => crate::core::value_to_string(f),
            }
        } else {
            re_flags
        };
        return create_regexp_object_from_parts(mc, env, re_pattern, flags, true);
    }

    // Step 5: Else if patternIsRegExp, read source/flags from regexp-like object
    let pattern_is_regexp = is_regexp(&arg0, env);
    if pattern_is_regexp && let Value::Object(obj) = &arg0 {
        // Step 5a: P = ? Get(pattern, "source")
        let source_val = if let Some(env) = env {
            crate::core::get_property_with_accessors(mc, env, obj, "source")?
        } else if let Some(v) = object_get_key_value(obj, "source") {
            v.borrow().clone()
        } else {
            Value::Undefined
        };
        let pattern_u16 = match &source_val {
            Value::String(s) => s.clone(),
            Value::Undefined => Vec::new(),
            other => utf8_to_utf16(&crate::core::value_to_string(other)),
        };

        // Step 5b-c: If flags undefined, F = ? Get(pattern, "flags"); else F = flags
        let flags = if let Some(f) = &arg1 {
            match f {
                Value::Undefined => {
                    // Read flags from object
                    let flags_val = if let Some(env) = env {
                        crate::core::get_property_with_accessors(mc, env, obj, "flags")?
                    } else if let Some(v) = object_get_key_value(obj, "flags") {
                        v.borrow().clone()
                    } else {
                        Value::Undefined
                    };
                    match &flags_val {
                        Value::String(s) => utf16_to_utf8(s),
                        Value::Undefined => String::new(),
                        other => crate::core::value_to_string(other),
                    }
                }
                Value::String(s) => utf16_to_utf8(s),
                Value::Object(_) => {
                    if let Some(env) = env {
                        let prim = crate::core::to_primitive(mc, f, "string", env)?;
                        crate::core::value_to_string(&prim)
                    } else {
                        crate::core::value_to_string(f)
                    }
                }
                _ => crate::core::value_to_string(f),
            }
        } else {
            // flags argument absent → read from object
            let flags_val = if let Some(env) = env {
                crate::core::get_property_with_accessors(mc, env, obj, "flags")?
            } else if let Some(v) = object_get_key_value(obj, "flags") {
                v.borrow().clone()
            } else {
                Value::Undefined
            };
            match &flags_val {
                Value::String(s) => utf16_to_utf8(s),
                Value::Undefined => String::new(),
                other => crate::core::value_to_string(other),
            }
        };

        return create_regexp_object_from_parts(mc, env, pattern_u16, flags, true);
    }

    // ToString coercion on pattern (undefined → empty)
    let pattern_u16 = match &arg0 {
        Value::Undefined => Vec::new(),
        Value::String(s) => s.clone(),
        Value::Object(_) => {
            if let Some(env) = env {
                let prim = crate::core::to_primitive(mc, &arg0, "string", env)?;
                match prim {
                    Value::String(s) => s,
                    other => utf8_to_utf16(&crate::core::value_to_string(&other)),
                }
            } else {
                utf8_to_utf16(&crate::core::value_to_string(&arg0))
            }
        }
        _ => value_to_pattern_u16(&arg0),
    };

    // ToString coercion on flags
    let flags = match &arg1 {
        Some(Value::String(s)) => utf16_to_utf8(s),
        Some(Value::Undefined) | None => String::new(),
        Some(Value::Object(_)) => {
            if let Some(env) = env {
                let prim = crate::core::to_primitive(mc, arg1.as_ref().unwrap(), "string", env)?;
                crate::core::value_to_string(&prim)
            } else {
                crate::core::value_to_string(arg1.as_ref().unwrap())
            }
        }
        Some(f) => crate::core::value_to_string(f),
    };

    create_regexp_object_from_parts(mc, env, pattern_u16, flags, true)
}

/// Handle RegExp() called as a function (without new).
/// Per spec §21.2.3.1: if pattern is RegExp, flags is undefined, and
/// pattern.constructor === RegExp, return the same object.
pub(crate) fn handle_regexp_call_with_env<'gc>(
    mc: &MutationContext<'gc>,
    env: Option<&JSObjectDataPtr<'gc>>,
    args: &[Value<'gc>],
) -> Result<Value<'gc>, EvalError<'gc>> {
    let arg0 = args.first().cloned().unwrap_or(Value::Undefined);
    let arg1 = args.get(1).cloned();

    // Step 1: Let patternIsRegExp = IsRegExp(pattern)
    let pattern_is_regexp = is_regexp(&arg0, env);

    // Step 4b: If patternIsRegExp is true and flags is undefined
    if pattern_is_regexp {
        let flags_undefined = matches!(&arg1, None | Some(Value::Undefined));
        if flags_undefined && let Value::Object(obj) = &arg0 {
            // Step 4b.i: Let patternConstructor = ? Get(pattern, "constructor")
            let ctor_val = if let Some(env) = env {
                crate::core::get_property_with_accessors(mc, env, obj, "constructor")?
            } else if let Some(v) = object_get_key_value(obj, "constructor") {
                v.borrow().clone()
            } else {
                Value::Undefined
            };

            // Step 4b.iii: If SameValue(newTarget, patternConstructor) is true, return pattern
            let ctor_is_regexp = if let Some(env) = env {
                if let Some(regexp_val) = object_get_key_value(env, "RegExp") {
                    match (&ctor_val, &*regexp_val.borrow()) {
                        (Value::Object(a), Value::Object(b)) => std::ptr::eq(a.as_ptr(), b.as_ptr()),
                        _ => false,
                    }
                } else {
                    false
                }
            } else {
                false
            };
            if ctor_is_regexp {
                return Ok(arg0);
            }
        }
    }

    // Otherwise, construct a new RegExp
    handle_regexp_constructor_with_env(mc, env, args)
}

/// Read the flags string from a RegExp object's internal Flags slot.
/// ToLength: clamp to integer in [0, 2^53-1] (for primitive values only)
fn to_length_primitive(val: &Value) -> usize {
    let n = match val {
        Value::Number(n) => {
            if n.is_nan() || *n <= 0.0 {
                return 0;
            }
            if n.is_infinite() {
                return (1usize << 53) - 1;
            }
            n.trunc() as usize
        }
        Value::String(s) => {
            let s_utf8 = utf16_to_utf8(s);
            match s_utf8.parse::<f64>() {
                Ok(n) if !n.is_nan() && n > 0.0 => n.trunc() as usize,
                _ => 0,
            }
        }
        Value::Boolean(b) => {
            if *b {
                1
            } else {
                0
            }
        }
        Value::Undefined | Value::Null => 0,
        _ => 0,
    };
    n.min((1usize << 53) - 1)
}

/// ToLength with ToPrimitive for objects: calls valueOf/toString on objects
fn to_length_with_coercion<'gc>(mc: &MutationContext<'gc>, val: &Value<'gc>, env: &JSObjectDataPtr<'gc>) -> Result<usize, EvalError<'gc>> {
    match val {
        Value::Object(_) => {
            let prim = crate::core::to_primitive(mc, val, "number", env)?;
            Ok(to_length_primitive(&prim))
        }
        _ => Ok(to_length_primitive(val)),
    }
}

/// Try to set lastIndex; throw TypeError if non-writable (strict mode)
fn set_last_index_checked<'gc>(mc: &MutationContext<'gc>, object: &JSObjectDataPtr<'gc>, value: f64) -> Result<(), EvalError<'gc>> {
    // Check if lastIndex is non-writable
    if object
        .borrow()
        .non_writable
        .contains(&crate::core::PropertyKey::String("lastIndex".to_string()))
    {
        return Err(raise_type_error!("Cannot set property lastIndex of [object Object] which has only a getter").into());
    }
    object_set_key_value(mc, object, "lastIndex", &Value::Number(value))?;
    Ok(())
}

fn internal_get_flags_string(object: &JSObjectDataPtr) -> String {
    match slot_get(object, &InternalSlot::Flags) {
        Some(val) => match &*val.borrow() {
            Value::String(s) => utf16_to_utf8(s),
            _ => String::new(),
        },
        None => String::new(),
    }
}

/// §22.2.5.2.1 RegExpExec abstract operation.
/// Checks for a user-defined `exec` property on the regexp object;
/// if callable, delegates to it. Otherwise falls back to built-in exec.
fn regexp_exec_abstract<'gc>(
    mc: &MutationContext<'gc>,
    rx: &JSObjectDataPtr<'gc>,
    string: &Value<'gc>,
    env: &JSObjectDataPtr<'gc>,
) -> Result<Value<'gc>, EvalError<'gc>> {
    // Step 1: Let exec be ? Get(R, "exec")
    let exec_val = crate::core::get_property_with_accessors(mc, env, rx, "exec")?;
    // Step 2: If IsCallable(exec), then
    let is_callable = match &exec_val {
        Value::Function(_) | Value::Closure(_) => true,
        Value::Object(o) => o.borrow().get_closure().is_some(),
        _ => false,
    };
    if is_callable {
        // Call exec with rx as this, [string] as args
        let result =
            crate::js_promise::call_function_with_this(mc, &exec_val, Some(&Value::Object(*rx)), std::slice::from_ref(string), env)?;
        // Step 2.a: Result must be null or object
        match &result {
            Value::Null | Value::Object(_) => return Ok(result),
            _ => return Err(raise_type_error!("RegExpExec: exec result must be null or an object").into()),
        }
    }
    // Step 3: Fall back to built-in exec
    handle_regexp_method(mc, rx, "exec", std::slice::from_ref(string), env)
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
            // Step 1: If this does not have [[RegExpMatcher]], throw TypeError
            if slot_get(object, &InternalSlot::Regex).is_none() {
                return Err(raise_type_error!("RegExp.prototype.exec called on incompatible receiver").into());
            }
            // Per spec: if no argument, use "undefined" string
            let input_val = if args.is_empty() {
                Value::String(utf8_to_utf16("undefined"))
            } else {
                args[0].clone()
            };
            // ToString coercion — calls toString()/valueOf() on objects
            let input_u16 = match input_val {
                Value::String(s) => s,
                Value::Number(n) => utf8_to_utf16(&crate::core::value_to_string(&Value::Number(n))),
                Value::Boolean(b) => utf8_to_utf16(&b.to_string()),
                Value::Undefined => utf8_to_utf16("undefined"),
                Value::Null => utf8_to_utf16("null"),
                Value::Object(_) => {
                    let prim = crate::core::to_primitive(mc, &input_val, "string", env)?;
                    match prim {
                        Value::String(s) => s,
                        other => utf8_to_utf16(&crate::core::value_to_string(&other)),
                    }
                }
                _ => utf8_to_utf16(&crate::core::value_to_string(&input_val)),
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

            // Per spec (22.2.5.2.2): Always read lastIndex via Get (observable)
            let last_index_val = crate::core::get_property_with_accessors(mc, env, object, "lastIndex").unwrap_or(Value::Number(0.0));
            // ToLength coercion (calls valueOf on objects)
            let raw_last_index = to_length_with_coercion(mc, &last_index_val, env)?;

            let last_index = if use_last {
                raw_last_index
            } else {
                // Non-global/non-sticky: always start from 0, but we still read lastIndex above (observable)
                0
            };

            let match_result = re.find_from_utf16(&working_input, last_index).next();
            // For sticky: match must start at exactly lastIndex
            let match_result = if sticky {
                match match_result {
                    Some(ref m) if m.range.start != last_index => None,
                    other => other,
                }
            } else {
                match_result
            };

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
                        set_last_index_checked(mc, object, orig_end as f64)?;
                    }

                    Ok(Value::Object(result_array))
                }
                None => {
                    if use_last {
                        set_last_index_checked(mc, object, 0.0)?;
                    }
                    Ok(Value::Null)
                }
            }
        }
        "test" => {
            // Step 1: If this does not have [[RegExpMatcher]], throw TypeError
            if slot_get(object, &InternalSlot::Regex).is_none() {
                return Err(raise_type_error!("RegExp.prototype.test called on incompatible receiver").into());
            }
            // Per spec: if no argument, use "undefined" string
            let input_val = if args.is_empty() {
                Value::String(utf8_to_utf16("undefined"))
            } else {
                args[0].clone()
            };
            // ToString coercion — calls toString()/valueOf() on objects
            let input_u16 = match input_val {
                Value::String(s) => s,
                Value::Number(n) => utf8_to_utf16(&crate::core::value_to_string(&Value::Number(n))),
                Value::Boolean(b) => utf8_to_utf16(&b.to_string()),
                Value::Undefined => utf8_to_utf16("undefined"),
                Value::Null => utf8_to_utf16("null"),
                Value::Object(_) => {
                    let prim = crate::core::to_primitive(mc, &input_val, "string", env)?;
                    match prim {
                        Value::String(s) => s,
                        other => utf8_to_utf16(&crate::core::value_to_string(&other)),
                    }
                }
                _ => utf8_to_utf16(&crate::core::value_to_string(&input_val)),
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

            // Per spec: Always read lastIndex via Get (observable), then ToLength
            let last_index_val = crate::core::get_property_with_accessors(mc, env, object, "lastIndex").unwrap_or(Value::Number(0.0));
            let raw_last_index = to_length_with_coercion(mc, &last_index_val, env)?;
            let last_index = if use_last { raw_last_index } else { 0 };

            let match_result = re.find_from_utf16(&working_input, last_index).next();
            // For sticky: match must start at exactly lastIndex
            let match_result = if sticky {
                match match_result {
                    Some(ref m) if m.range.start != last_index => None,
                    other => other,
                }
            } else {
                match_result
            };

            match match_result {
                Some(m) => {
                    if use_last {
                        let end = m.range.end;
                        let orig_end = if mapping { map_index_back(&input_u16, end) } else { end };
                        set_last_index_checked(mc, object, orig_end as f64)?;
                    }
                    Ok(Value::Boolean(true))
                }
                None => {
                    if use_last {
                        set_last_index_checked(mc, object, 0.0)?;
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
        "match" => {
            // §22.2.5.6 RegExp.prototype[@@match](string)
            // Step 3: Let S = ? ToString(string)
            let input_str = if args.is_empty() {
                utf8_to_utf16("undefined")
            } else {
                match &args[0] {
                    Value::String(s) => s.clone(),
                    Value::Object(_) => {
                        let prim = crate::core::to_primitive(mc, &args[0], "string", env)?;
                        match prim {
                            Value::String(s) => s,
                            other => utf8_to_utf16(&crate::core::value_to_string(&other)),
                        }
                    }
                    other => utf8_to_utf16(&crate::core::value_to_string(other)),
                }
            };

            // Step 4: Let flags = ? ToString(? Get(rx, "flags"))
            let flags_val = crate::core::get_property_with_accessors(mc, env, object, "flags")?;
            let flags_str = match &flags_val {
                Value::String(s) => utf16_to_utf8(s),
                Value::Object(_) => {
                    let prim = crate::core::to_primitive(mc, &flags_val, "string", env)?;
                    match prim {
                        Value::String(s) => utf16_to_utf8(&s),
                        other => crate::core::value_to_string(&other),
                    }
                }
                other => crate::core::value_to_string(other),
            };
            let global = flags_str.contains('g');

            if !global {
                // Step 5: Non-global: just call exec and return its result
                return regexp_exec_abstract(mc, object, &Value::String(input_str), env);
            }

            // Step 6: Global match
            let full_unicode = flags_str.contains('u');

            // Step 6b: Set lastIndex = 0
            set_last_index_checked(mc, object, 0.0)?;

            let result_array = create_array(mc, env)?;
            let mut n = 0usize;

            loop {
                // Step 6e.i: Let result = ? RegExpExec(rx, S)
                let exec_result = regexp_exec_abstract(mc, object, &Value::String(input_str.clone()), env)?;
                if matches!(exec_result, Value::Null) {
                    if n == 0 {
                        return Ok(Value::Null);
                    }
                    set_array_length(mc, &result_array, n)?;
                    return Ok(Value::Object(result_array));
                }

                // Step 6e.iii: Let matchStr = ? ToString(? Get(result, "0"))
                let match_str_val = if let Value::Object(res_obj) = &exec_result {
                    crate::core::get_property_with_accessors(mc, env, res_obj, "0")?
                } else {
                    Value::Undefined
                };
                let match_str = match &match_str_val {
                    Value::String(s) => s.clone(),
                    Value::Object(_) => {
                        let prim = crate::core::to_primitive(mc, &match_str_val, "string", env)?;
                        match prim {
                            Value::String(s) => s,
                            other => utf8_to_utf16(&crate::core::value_to_string(&other)),
                        }
                    }
                    other => utf8_to_utf16(&crate::core::value_to_string(other)),
                };

                // Step 6e.iv
                object_set_key_value(mc, &result_array, n, &Value::String(match_str.clone()))?;

                // Step 6e.v: If matchStr is empty, advance lastIndex
                if match_str.is_empty() {
                    let this_index_val = crate::core::get_property_with_accessors(mc, env, object, "lastIndex")?;
                    let this_index = to_length_with_coercion(mc, &this_index_val, env)?;
                    let next_index = if full_unicode {
                        advance_string_index_unicode(&input_str, this_index)
                    } else {
                        this_index + 1
                    };
                    set_last_index_checked(mc, object, next_index as f64)?;
                }

                n += 1;
                if n > 1_000_000 {
                    break;
                }
            }
            set_array_length(mc, &result_array, n)?;
            Ok(Value::Object(result_array))
        }
        "replace" => {
            // §22.2.5.8 RegExp.prototype[@@replace](string, replaceValue)
            // Step 3: Let string = ? ToString(string)
            let input_str = if args.is_empty() {
                utf8_to_utf16("undefined")
            } else {
                match &args[0] {
                    Value::String(s) => s.clone(),
                    Value::Object(_) => {
                        let prim = crate::core::to_primitive(mc, &args[0], "string", env)?;
                        match prim {
                            Value::String(s) => s,
                            other => utf8_to_utf16(&crate::core::value_to_string(&other)),
                        }
                    }
                    other => utf8_to_utf16(&crate::core::value_to_string(other)),
                }
            };

            let replace_value = args.get(1).cloned().unwrap_or(Value::Undefined);

            // Step 5: Let flags = ? ToString(? Get(rx, "flags"))
            let flags_val = crate::core::get_property_with_accessors(mc, env, object, "flags")?;
            let flags_str = match &flags_val {
                Value::String(s) => utf16_to_utf8(s),
                Value::Object(_) => {
                    let prim = crate::core::to_primitive(mc, &flags_val, "string", env)?;
                    match prim {
                        Value::String(s) => utf16_to_utf8(&s),
                        other => crate::core::value_to_string(&other),
                    }
                }
                other => crate::core::value_to_string(other),
            };
            let global = flags_str.contains('g');
            let full_unicode = flags_str.contains('u');

            if global {
                set_last_index_checked(mc, object, 0.0)?;
            }

            // Collect all match results
            let mut results: Vec<Value<'gc>> = Vec::new();
            loop {
                let exec_result = regexp_exec_abstract(mc, object, &Value::String(input_str.clone()), env)?;
                if matches!(exec_result, Value::Null) {
                    break;
                }
                results.push(exec_result.clone());

                if !global {
                    break;
                }

                // Advance lastIndex if empty match
                if let Value::Object(res_obj) = &exec_result {
                    let match_val = crate::core::get_property_with_accessors(mc, env, res_obj, "0").unwrap_or(Value::Undefined);
                    let match_str = match &match_val {
                        Value::String(s) => s.clone(),
                        Value::Object(_) => {
                            let prim = crate::core::to_primitive(mc, &match_val, "string", env)?;
                            match prim {
                                Value::String(s) => s,
                                other => utf8_to_utf16(&crate::core::value_to_string(&other)),
                            }
                        }
                        other => utf8_to_utf16(&crate::core::value_to_string(other)),
                    };

                    if match_str.is_empty() {
                        let this_index_val =
                            crate::core::get_property_with_accessors(mc, env, object, "lastIndex").unwrap_or(Value::Number(0.0));
                        let this_index = to_length_with_coercion(mc, &this_index_val, env)?;
                        let next_index = if full_unicode {
                            advance_string_index_unicode(&input_str, this_index)
                        } else {
                            this_index + 1
                        };
                        set_last_index_checked(mc, object, next_index as f64)?;
                    }
                }

                if results.len() > 1_000_000 {
                    break;
                }
            }

            // Check if replaceValue is callable
            let replace_is_fn = matches!(&replace_value, Value::Object(o) if {
                let b = o.borrow();
                b.get_closure().is_some() ||
                crate::core::slot_get(o, &InternalSlot::Callable).is_some() ||
                crate::core::slot_get(o, &InternalSlot::Function).is_some()
            }) || matches!(&replace_value, Value::Function(_) | Value::Closure(_));

            // If not callable, coerce replaceValue to string
            let replace_str_val = if !replace_is_fn {
                match &replace_value {
                    Value::String(s) => s.clone(),
                    Value::Object(_) => {
                        let prim = crate::core::to_primitive(mc, &replace_value, "string", env)?;
                        match prim {
                            Value::String(s) => s,
                            other => utf8_to_utf16(&crate::core::value_to_string(&other)),
                        }
                    }
                    other => utf8_to_utf16(&crate::core::value_to_string(other)),
                }
            } else {
                Vec::new()
            };

            // Build result string
            let mut acc_next_source_position = 0usize;
            let mut accumulated = Vec::<u16>::new();

            for result in &results {
                let result_obj = match result {
                    Value::Object(o) => o,
                    _ => continue,
                };

                // Step 14a: Let nCaptures = ? ToLength(? Get(result, "length"))
                let n_captures_val = crate::core::get_property_with_accessors(mc, env, result_obj, "length")?;
                let n_captures_raw = to_length_with_coercion(mc, &n_captures_val, env)?;
                let n_captures = if n_captures_raw > 0 { n_captures_raw - 1 } else { 0 };

                // Step 14c: Let matched = ? ToString(? Get(result, "0"))
                let matched_val = crate::core::get_property_with_accessors(mc, env, result_obj, "0")?;
                let matched = match &matched_val {
                    Value::String(s) => s.clone(),
                    Value::Object(_) => {
                        let prim = crate::core::to_primitive(mc, &matched_val, "string", env)?;
                        match prim {
                            Value::String(s) => s,
                            other => utf8_to_utf16(&crate::core::value_to_string(&other)),
                        }
                    }
                    other => utf8_to_utf16(&crate::core::value_to_string(other)),
                };

                // Step 14d: Let position = ? ToIntegerOrInfinity(? Get(result, "index"))
                let position_val = crate::core::get_property_with_accessors(mc, env, result_obj, "index")?;
                let position = match &position_val {
                    Value::Number(n) => (*n as isize).max(0) as usize,
                    Value::Undefined => 0,
                    Value::Object(_) => {
                        let prim = crate::core::to_primitive(mc, &position_val, "number", env)?;
                        match prim {
                            Value::Number(n) => (n as isize).max(0) as usize,
                            _ => 0,
                        }
                    }
                    _ => {
                        let s = crate::core::value_to_string(&position_val);
                        s.parse::<f64>().unwrap_or(0.0).max(0.0) as usize
                    }
                };
                let position = position.min(input_str.len());

                // Step 14e-g: Collect captures
                let mut captures: Vec<Value<'gc>> = Vec::new();
                for i in 1..=n_captures {
                    let cap_val = crate::core::get_property_with_accessors(mc, env, result_obj, i)?;
                    if matches!(cap_val, Value::Undefined) {
                        captures.push(Value::Undefined);
                    } else {
                        let cap_str = match &cap_val {
                            Value::String(s) => Value::String(s.clone()),
                            Value::Object(_) => {
                                let prim = crate::core::to_primitive(mc, &cap_val, "string", env)?;
                                match prim {
                                    Value::String(s) => Value::String(s),
                                    other => Value::String(utf8_to_utf16(&crate::core::value_to_string(&other))),
                                }
                            }
                            other => Value::String(utf8_to_utf16(&crate::core::value_to_string(other))),
                        };
                        captures.push(cap_str);
                    }
                }

                // Step 14h: Get named captures
                let named_captures_val = crate::core::get_property_with_accessors(mc, env, result_obj, "groups")?;
                let named_captures = if matches!(named_captures_val, Value::Undefined) {
                    None
                } else {
                    // Step 14l.i: If namedCaptures is not undefined, Set namedCaptures to ? ToObject(namedCaptures).
                    // ToObject(null) throws TypeError
                    if !replace_is_fn {
                        match &named_captures_val {
                            Value::Null => {
                                return Err(raise_type_error!("Cannot convert null to object").into());
                            }
                            Value::Object(_) => Some(named_captures_val),
                            Value::String(s) => {
                                // ToObject wraps string in a String object
                                let str_obj = new_js_object_data(mc);
                                let s_clone = s.clone();
                                object_set_key_value(mc, &str_obj, "length", &Value::Number(s_clone.len() as f64))?;
                                for (idx, &ch) in s_clone.iter().enumerate() {
                                    object_set_key_value(mc, &str_obj, idx, &Value::String(vec![ch]))?;
                                }
                                Some(Value::Object(str_obj))
                            }
                            Value::Number(_) | Value::Boolean(_) => {
                                // ToObject wraps in Number/Boolean object — no useful properties for GetSubstitution
                                let wrap_obj = new_js_object_data(mc);
                                Some(Value::Object(wrap_obj))
                            }
                            _ => Some(named_captures_val),
                        }
                    } else {
                        Some(named_captures_val)
                    }
                };

                let replacement: Vec<u16> = if replace_is_fn {
                    // Call replaceValue function
                    let mut call_args = vec![Value::String(matched.clone())];
                    for cap in &captures {
                        call_args.push(cap.clone());
                    }
                    call_args.push(Value::Number(position as f64));
                    call_args.push(Value::String(input_str.clone()));
                    if let Some(nc) = &named_captures {
                        call_args.push(nc.clone());
                    }
                    let rep_val = crate::js_promise::call_function(mc, &replace_value, &call_args, env)?;
                    match rep_val {
                        Value::String(s) => s,
                        Value::Object(_) => {
                            let prim = crate::core::to_primitive(mc, &rep_val, "string", env)?;
                            match prim {
                                Value::String(s) => s,
                                other => utf8_to_utf16(&crate::core::value_to_string(&other)),
                            }
                        }
                        other => utf8_to_utf16(&crate::core::value_to_string(&other)),
                    }
                } else {
                    // GetSubstitution
                    get_substitution(
                        &matched,
                        &input_str,
                        position,
                        &captures,
                        &named_captures,
                        &replace_str_val,
                        mc,
                        env,
                    )?
                };

                // Append non-matched portion + replacement
                if position >= acc_next_source_position {
                    accumulated.extend_from_slice(&input_str[acc_next_source_position..position]);
                    accumulated.extend_from_slice(&replacement);
                    acc_next_source_position = position + matched.len();
                }
            }

            // Append remaining portion of input
            if acc_next_source_position < input_str.len() {
                accumulated.extend_from_slice(&input_str[acc_next_source_position..]);
            }

            Ok(Value::String(accumulated))
        }
        "search" => {
            // §21.2.5.9 RegExp.prototype[@@search](string)
            let input_str = if args.is_empty() {
                utf8_to_utf16("undefined")
            } else {
                match &args[0] {
                    Value::String(s) => s.clone(),
                    other => utf8_to_utf16(&crate::core::value_to_string(other)),
                }
            };

            // Save lastIndex
            let previous_last_index = if let Some(li) = object_get_key_value(object, "lastIndex") {
                li.borrow().clone()
            } else {
                Value::Number(0.0)
            };

            object_set_key_value(mc, object, "lastIndex", &Value::Number(0.0))?;

            let exec_result = regexp_exec_abstract(mc, object, &Value::String(input_str), env)?;

            // Restore lastIndex
            object_set_key_value(mc, object, "lastIndex", &previous_last_index)?;

            if matches!(exec_result, Value::Null) {
                Ok(Value::Number(-1.0))
            } else if let Value::Object(res_obj) = &exec_result {
                if let Some(v) = object_get_key_value(res_obj, "index") {
                    Ok(v.borrow().clone())
                } else {
                    Ok(Value::Number(-1.0))
                }
            } else {
                Ok(Value::Number(-1.0))
            }
        }
        "split" => {
            // §21.2.5.11 RegExp.prototype[@@split](string, limit)
            let input_str = if args.is_empty() {
                utf8_to_utf16("undefined")
            } else {
                match &args[0] {
                    Value::String(s) => s.clone(),
                    other => utf8_to_utf16(&crate::core::value_to_string(other)),
                }
            };

            let limit = if let Some(lim) = args.get(1) {
                match lim {
                    Value::Undefined => u32::MAX,
                    Value::Number(n) => *n as u32,
                    _ => u32::MAX,
                }
            } else {
                u32::MAX
            };

            let result_array = create_array(mc, env)?;
            if limit == 0 {
                set_array_length(mc, &result_array, 0)?;
                return Ok(Value::Object(result_array));
            }

            let flags_str = internal_get_flags_string(object);
            let full_unicode = flags_str.contains('u') || flags_str.contains('v');

            // Build a "sticky" regex for splitting (spec Step 7: add "y" flag)
            let pattern_u16 = internal_get_regex_pattern(object)?;
            let mut r_flags = String::new();
            for c in flags_str.chars() {
                if "gimsuy".contains(c) {
                    r_flags.push(c);
                }
                if c == 'v' {
                    r_flags.push('u');
                }
            }
            if !r_flags.contains('y') {
                r_flags.push('y');
            }
            let re = create_regex_from_utf16(&pattern_u16, &r_flags)
                .map_err(|e| raise_syntax_error!(format!("Invalid RegExp in split: {e}")))?;

            let str_len = input_str.len();

            if input_str.is_empty() {
                // Empty string: if regex matches empty string, return []; else return [""]
                if re.find_from_utf16(&input_str, 0).next().is_some() {
                    set_array_length(mc, &result_array, 0)?;
                } else {
                    object_set_key_value(mc, &result_array, 0usize, &Value::String(input_str))?;
                    set_array_length(mc, &result_array, 1)?;
                }
                return Ok(Value::Object(result_array));
            }

            let mut p = 0usize; // end of last match
            let mut arr_len = 0usize;
            let mut q = p;

            while q < str_len {
                // Try sticky match at position q
                let m = re.find_from_utf16(&input_str, q).next();
                let m = match m {
                    Some(m) if m.range.start == q => m, // sticky: must match at q
                    _ => {
                        q = if full_unicode {
                            advance_string_index_unicode(&input_str, q)
                        } else {
                            q + 1
                        };
                        continue;
                    }
                };

                let e = m.range.end;
                if e == p {
                    // Empty match at same position as last split point — advance
                    q = if full_unicode {
                        advance_string_index_unicode(&input_str, q)
                    } else {
                        q + 1
                    };
                    continue;
                }

                // Add substring before match: input[p..q]
                let sub = input_str[p..q].to_vec();
                object_set_key_value(mc, &result_array, arr_len, &Value::String(sub))?;
                arr_len += 1;
                if arr_len as u32 >= limit {
                    set_array_length(mc, &result_array, arr_len)?;
                    return Ok(Value::Object(result_array));
                }

                // Add captures from the match
                for cap in m.captures.iter() {
                    if let Some(range) = cap {
                        let cap_str = input_str[range.start..range.end].to_vec();
                        object_set_key_value(mc, &result_array, arr_len, &Value::String(cap_str))?;
                    } else {
                        object_set_key_value(mc, &result_array, arr_len, &Value::Undefined)?;
                    }
                    arr_len += 1;
                    if arr_len as u32 >= limit {
                        set_array_length(mc, &result_array, arr_len)?;
                        return Ok(Value::Object(result_array));
                    }
                }

                p = e;
                q = p;
            }

            // Add tail: input[p..str_len]
            let sub = input_str[p..str_len].to_vec();
            object_set_key_value(mc, &result_array, arr_len, &Value::String(sub))?;
            arr_len += 1;
            set_array_length(mc, &result_array, arr_len)?;
            Ok(Value::Object(result_array))
        }
        _ => Err(raise_eval_error!(format!("RegExp.prototype.{method} is not implemented")).into()),
    }
}

/// Advance string index by one code point (surrogate-aware for Unicode mode).
fn advance_string_index_unicode(s: &[u16], index: usize) -> usize {
    if index + 1 >= s.len() {
        return index + 1;
    }
    let first = s[index];
    if (0xD800..=0xDBFF).contains(&first) {
        let second = s[index + 1];
        if (0xDC00..=0xDFFF).contains(&second) {
            return index + 2;
        }
    }
    index + 1
}

/// §21.1.3.17.1 GetSubstitution(matched, str, position, captures, namedCaptures, replacementTemplate)
#[allow(clippy::too_many_arguments)]
fn get_substitution<'gc>(
    matched: &[u16],
    string: &[u16],
    position: usize,
    captures: &[Value<'gc>],
    named_captures: &Option<Value<'gc>>,
    replacement: &[u16],
    mc: &MutationContext<'gc>,
    env: &JSObjectDataPtr<'gc>,
) -> Result<Vec<u16>, EvalError<'gc>> {
    let mut result = Vec::new();
    let tail_pos = (position + matched.len()).min(string.len());
    let m = captures.len();
    let mut i = 0;

    while i < replacement.len() {
        let ch = replacement[i];
        if ch == '$' as u16 && i + 1 < replacement.len() {
            let next = replacement[i + 1];
            match next {
                // $$ → literal $
                c if c == '$' as u16 => {
                    result.push('$' as u16);
                    i += 2;
                }
                // $& → matched substring
                c if c == '&' as u16 => {
                    result.extend_from_slice(matched);
                    i += 2;
                }
                // $` → portion before match
                c if c == '`' as u16 => {
                    if position > 0 {
                        result.extend_from_slice(&string[..position]);
                    }
                    i += 2;
                }
                // $' → portion after match
                c if c == '\'' as u16 => {
                    if tail_pos < string.len() {
                        result.extend_from_slice(&string[tail_pos..]);
                    }
                    i += 2;
                }
                // $<name> → named capture
                c if c == '<' as u16 => {
                    if named_captures.is_none() {
                        // namedCaptures is undefined → literal $<
                        result.push('$' as u16);
                        result.push('<' as u16);
                        i += 2;
                    } else if let Some(gt_pos) = replacement[i + 2..].iter().position(|&u| u == '>' as u16) {
                        let name_u16 = &replacement[i + 2..i + 2 + gt_pos];
                        let name = utf16_to_utf8(name_u16);
                        if let Some(Value::Object(groups)) = named_captures {
                            let capture_val = crate::core::get_property_with_accessors(mc, env, groups, &*name)?;
                            match &capture_val {
                                Value::Undefined => {} // empty replacement
                                Value::String(s) => result.extend_from_slice(s),
                                Value::Object(_) => {
                                    let prim = crate::core::to_primitive(mc, &capture_val, "string", env)?;
                                    match prim {
                                        Value::String(s) => result.extend_from_slice(&s),
                                        other => {
                                            let s = crate::core::value_to_string(&other);
                                            result.extend_from_slice(&utf8_to_utf16(&s));
                                        }
                                    }
                                }
                                other => {
                                    let s = crate::core::value_to_string(other);
                                    result.extend_from_slice(&utf8_to_utf16(&s));
                                }
                            }
                        }
                        i += 2 + gt_pos + 1;
                    } else {
                        // No matching > found
                        result.push(ch);
                        i += 1;
                    }
                }
                // $n or $nn → numbered capture
                c if (c as u8).is_ascii_digit() => {
                    let d1 = (c as u8 - b'0') as usize;
                    // Check for two-digit reference $nn
                    if i + 2 < replacement.len() {
                        let next2 = replacement[i + 2];
                        if (next2 as u8).is_ascii_digit() {
                            let d2 = (next2 as u8 - b'0') as usize;
                            let nn = d1 * 10 + d2;
                            if nn >= 1 && nn <= m {
                                match &captures[nn - 1] {
                                    Value::String(s) => result.extend_from_slice(s),
                                    Value::Undefined => {}
                                    other => {
                                        let s = crate::core::value_to_string(other);
                                        result.extend_from_slice(&utf8_to_utf16(&s));
                                    }
                                }
                                i += 3;
                                continue;
                            }
                        }
                    }
                    // Single digit
                    if d1 >= 1 && d1 <= m {
                        match &captures[d1 - 1] {
                            Value::String(s) => result.extend_from_slice(s),
                            Value::Undefined => {}
                            other => {
                                let s = crate::core::value_to_string(other);
                                result.extend_from_slice(&utf8_to_utf16(&s));
                            }
                        }
                        i += 2;
                    } else {
                        result.push('$' as u16);
                        result.push(c);
                        i += 2;
                    }
                }
                _ => {
                    result.push(ch);
                    i += 1;
                }
            }
        } else {
            result.push(ch);
            i += 1;
        }
    }

    Ok(result)
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
